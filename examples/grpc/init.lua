local fiber = require('fiber')
local grpc = require('grpc')

box.cfg {
    wal_mode = 'none',
}

local space = box.schema.create_space('users', { if_not_exists = true })
space:format {
    {'uuid', 'string'},
    {'username', 'string'},
}
space:create_index('pk', {
    if_not_exists = true,
    parts = {
        {1, 'string'},
        {2, 'string'},
    }
})

fiber.create(grpc.start, { buffer = 128 })
