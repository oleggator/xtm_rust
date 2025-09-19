local yaml = require('yaml')

box.cfg {
    wal_mode = 'none',
}
local space = box.schema.space.create('some_space', {
    if_not_exists = true,
})
space:create_index('pk', {
    parts = { { 1, 'unsigned' } },
    if_not_exists = true,
})

local simple = require('simple')
simple.start({
    fibers = 2,
})

print(yaml.encode(space:select()))
require('os').exit()
