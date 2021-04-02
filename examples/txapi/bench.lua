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

print('Rust module:')
local txapi = require('libtxapi')
txapi.start(16)

print('Lua:')
local clock = require('clock')
local tuple = { 1, 'some_string' }
local iteration = 1000000

-- TODO fix result calculation
local begin = clock.time64() * 1000
for _ = 1, iteration do
    space:replace(tuple)
end
local elapsed = clock.time64() * 1000 - begin
local per_cycle = elapsed / iteration
print('iteration:', iteration, 'elapsed:', elapsed, 'avg per cycle:', per_cycle)

require('console'):start()
