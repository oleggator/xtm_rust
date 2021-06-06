box.cfg {
    wal_mode = 'none',
    log_level = 0,
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
txapi.start(128)

print('Lua:')
local clock = require('clock')
local iteration = 1000000

local begin = clock.time64()
for i = 1000000, iteration + 1000000 do
    space:replace { i, 'some_string' }
end
local elapsed = clock.time64() - begin
local per_cycle = elapsed / iteration
print('iterations: ' .. tostring(iteration) .. ' | elapsed: ' .. tostring(elapsed) .. 'ns | avg per cycle: ' .. tostring(per_cycle) .. 'ns')

require('console'):start()
