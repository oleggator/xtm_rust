local txapi = require('bench')
txapi.start({
    fibers = 4,
    max_batch = 8,
    runtime = { type = "cur_thread" },
}) -- will use default
-- txapi.start({buffer = 128 })
-- txapi.start({buffer = 128, runtime = { type = "cur_thread" }})
-- txapi.start({buffer = 128, runtime = { type = "multi_thread" }}) -- default
-- txapi.start({buffer = 128, runtime = { type = "multi_thread", thread_count = 16 }})
