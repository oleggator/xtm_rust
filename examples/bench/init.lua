local txapi = require('bench')
txapi.start({
    fibers = 16,
    max_batch = 16,
    runtime = { type = "cur_thread" },
}) -- will use default
-- txapi.start({buffer = 128 })
-- txapi.start({buffer = 128, runtime = { type = "cur_thread" }})
-- txapi.start({buffer = 128, runtime = { type = "multi_thread" }}) -- default
-- txapi.start({buffer = 128, runtime = { type = "multi_thread", thread_count = 16 }})
