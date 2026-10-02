#include <assert.h>
#include <bare.h>
#include <js.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <utf.h>
#include <uv.h>

#include "simple_converter.h"

typedef struct {
  const uint8_t *input;
  size_t input_len;
  uint8_t *owned_input;
  char *from;
  char *to;
  char *fonts_dir;
  sc_buffer_t out;
  sc_buffer_t error;
  int status;
} simple_converter_job_t;

typedef struct {
  uv_work_t work;
  js_env_t *env;
  js_deferred_t *deferred;
  js_deferred_teardown_t *teardown;
  bool exiting;
  simple_converter_job_t job;
} simple_converter_req_t;

static bool
simple_converter__string(js_env_t *env, js_value_t *value, char **result) {
  int err;

  bool is_string;
  err = js_is_string(env, value, &is_string);
  if (err != 0 || !is_string) return false;

  size_t len = 0;
  err = js_get_value_string_utf8(env, value, NULL, 0, &len);
  if (err != 0) return false;

  char *str = malloc(len + 1);
  if (str == NULL) return false;

  err = js_get_value_string_utf8(env, value, (utf8_t *) str, len + 1, &len);
  if (err != 0) {
    free(str);
    return false;
  }

  str[len] = '\0';

  if (strlen(str) != len) {
    free(str);
    return false;
  }

  *result = str;
  return true;
}

static void
simple_converter__job_release(simple_converter_job_t *job) {
  free(job->owned_input);
  free(job->from);
  free(job->to);
  free(job->fonts_dir);
  sc_free(&job->out);
  sc_free(&job->error);
}

static bool
simple_converter__job_init(js_env_t *env, js_callback_info_t *info, bool copy_input, simple_converter_job_t *job) {
  int err;

  memset(job, 0, sizeof(*job));

  size_t argc = 4;
  js_value_t *argv[4];

  err = js_get_callback_info(env, info, &argc, argv, NULL, NULL);
  assert(err == 0);

  if (argc < 4) {
    err = js_throw_error(env, NULL, "convert expects (bytes, from, to, fontsDir)");
    assert(err == 0);
    return false;
  }

  bool is_typedarray;
  err = js_is_typedarray(env, argv[0], &is_typedarray);
  assert(err == 0);

  js_typedarray_type_t type = js_int8array;
  uint8_t *input = NULL;
  size_t input_len = 0;
  if (is_typedarray) {
    err = js_get_typedarray_info(env, argv[0], &type, (void **) &input, &input_len, NULL, NULL);
  }
  if (!is_typedarray || err != 0 || type != js_uint8array) {
    err = js_throw_error(env, NULL, "input must be a Uint8Array");
    assert(err == 0);
    return false;
  }

  bool ok = simple_converter__string(env, argv[1], &job->from) &&
            simple_converter__string(env, argv[2], &job->to) &&
            simple_converter__string(env, argv[3], &job->fonts_dir);

  if (!ok) {
    simple_converter__job_release(job);
    err = js_throw_error(env, NULL, "from, to and fontsDir must be strings without NUL characters");
    assert(err == 0);
    return false;
  }

  job->input = input;
  job->input_len = input_len;

  if (copy_input && input_len > 0) {
    job->owned_input = malloc(input_len);
    if (job->owned_input == NULL) {
      simple_converter__job_release(job);
      err = js_throw_error(env, NULL, "out of memory");
      assert(err == 0);
      return false;
    }
    memcpy(job->owned_input, input, input_len);
    job->input = job->owned_input;
  }

  return true;
}

static void
simple_converter__run(simple_converter_job_t *job) {
  job->status = sc_convert(job->input, job->input_len, job->from, job->to, job->fonts_dir, &job->out, &job->error);
}

static void
simple_converter__finalize_output(js_env_t *env, void *data, void *finalize_hint) {
  sc_buffer_t *buffer = finalize_hint;
  sc_free(buffer);
  free(buffer);
}

static js_value_t *
simple_converter__output(js_env_t *env, simple_converter_job_t *job) {
  int err;

  js_value_t *result;

  sc_buffer_t *owned = job->out.len > 0 ? malloc(sizeof(sc_buffer_t)) : NULL;

  if (owned != NULL) {
    *owned = job->out;
    err = js_create_external_arraybuffer(env, owned->data, owned->len, simple_converter__finalize_output, owned, &result);
    if (err == 0) {
      job->out.data = NULL;
      job->out.len = 0;
      return result;
    }
    free(owned);
  }

  void *data;
  err = js_create_arraybuffer(env, job->out.len, &data, &result);
  assert(err == 0);

  if (job->out.len > 0) memcpy(data, job->out.data, job->out.len);

  return result;
}

static js_value_t *
simple_converter__error(js_env_t *env, simple_converter_job_t *job) {
  int err;

  js_value_t *message;
  if (job->error.len > 0) {
    err = js_create_string_utf8(env, (utf8_t *) job->error.data, job->error.len, &message);
  } else {
    err = js_create_string_utf8(env, (utf8_t *) "conversion failed", (size_t) -1, &message);
  }
  assert(err == 0);

  js_value_t *error;
  err = js_create_error(env, NULL, message, &error);
  assert(err == 0);

  return error;
}

static js_value_t *
simple_converter_convert_sync(js_env_t *env, js_callback_info_t *info) {
  int err;

  simple_converter_job_t job;
  if (!simple_converter__job_init(env, info, false, &job)) return NULL;

  simple_converter__run(&job);

  js_value_t *result = NULL;

  if (job.status == 0) {
    result = simple_converter__output(env, &job);
  } else {
    err = js_throw(env, simple_converter__error(env, &job));
    assert(err == 0);
  }

  simple_converter__job_release(&job);

  return result;
}

static void
simple_converter__on_work(uv_work_t *work) {
  simple_converter_req_t *req = (simple_converter_req_t *) work;

  simple_converter__run(&req->job);
}

static void
simple_converter__on_after_work(uv_work_t *work, int status) {
  int err;

  simple_converter_req_t *req = (simple_converter_req_t *) work;

  js_env_t *env = req->env;

  if (!req->exiting) {
    js_handle_scope_t *scope;
    err = js_open_handle_scope(env, &scope);
    assert(err == 0);

    if (status == 0 && req->job.status == 0) {
      err = js_resolve_deferred(env, req->deferred, simple_converter__output(env, &req->job));
    } else {
      err = js_reject_deferred(env, req->deferred, simple_converter__error(env, &req->job));
    }
    assert(err == 0);

    err = js_close_handle_scope(env, scope);
    assert(err == 0);
  }

  err = js_finish_deferred_teardown_callback(req->teardown);
  assert(err == 0);

  simple_converter__job_release(&req->job);
  free(req);
}

static void
simple_converter__on_teardown(js_deferred_teardown_t *teardown, void *data) {
  simple_converter_req_t *req = data;

  req->exiting = true;

  uv_cancel((uv_req_t *) &req->work);
}

static js_value_t *
simple_converter_convert(js_env_t *env, js_callback_info_t *info) {
  int err;

  simple_converter_req_t *req = malloc(sizeof(simple_converter_req_t));
  if (req == NULL) {
    err = js_throw_error(env, NULL, "out of memory");
    assert(err == 0);
    return NULL;
  }

  if (!simple_converter__job_init(env, info, true, &req->job)) {
    free(req);
    return NULL;
  }

  req->env = env;
  req->exiting = false;

  js_value_t *promise;
  err = js_create_promise(env, &req->deferred, &promise);
  assert(err == 0);

  uv_loop_t *loop;
  err = js_get_env_loop(env, &loop);
  assert(err == 0);

  err = uv_queue_work(loop, &req->work, simple_converter__on_work, simple_converter__on_after_work);
  assert(err == 0);

  err = js_add_deferred_teardown_callback(env, simple_converter__on_teardown, req, &req->teardown);
  assert(err == 0);

  return promise;
}

static js_value_t *
simple_converter_exports(js_env_t *env, js_value_t *exports) {
  int err;

#define V(name, fn) \
  { \
    js_value_t *val; \
    err = js_create_function(env, name, -1, fn, NULL, &val); \
    assert(err == 0); \
    err = js_set_named_property(env, exports, name, val); \
    assert(err == 0); \
  }

  V("convert", simple_converter_convert)
  V("convertSync", simple_converter_convert_sync)
#undef V

  return exports;
}

BARE_MODULE(simple_converter, simple_converter_exports)
