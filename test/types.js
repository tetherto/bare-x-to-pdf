const test = require('brittle')
const { isMimeTypeSupported } = require('../types')

test('document MIME types', (t) => {
  t.ok(isMimeTypeSupported('application/msword'))
  t.ok(isMimeTypeSupported('application/vnd.ms-excel'))
  t.ok(
    isMimeTypeSupported('application/vnd.openxmlformats-officedocument.presentationml.presentation')
  )
  t.ok(isMimeTypeSupported('application/vnd.openxmlformats-officedocument.presentationml.template'))
  t.ok(isMimeTypeSupported('application/vnd.ms-powerpoint.presentation.macroenabled.12'))
  t.absent(isMimeTypeSupported('application/vnd.ms-powerpoint'))
  t.absent(isMimeTypeSupported('application/octet-stream'))
  t.absent(isMimeTypeSupported('docx'))
  t.absent(isMimeTypeSupported(null))
})
