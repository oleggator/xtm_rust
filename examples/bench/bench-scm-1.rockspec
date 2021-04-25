package = 'bench'
version = 'scm-1'
source  = {
    url    = '',
    branch = 'master',
}
description = {
    summary  = "",
    homepage = '',
    license  = 'MIT',
}
dependencies = {
    'lua >= 5.1',
}
external_dependencies = {
    TARANTOOL = {
        header = "tarantool/module.h"
    },
}
build = {
    type = 'make',

    variables = {
        version = 'scm-1',
        TARANTOOL_DIR = '$(TARANTOOL_DIR)',
        TARANTOOL_INSTALL_LIBDIR = '$(LIBDIR)',
        TARANTOOL_INSTALL_LUADIR = '$(LUADIR)',
    }
}

-- vim: syntax=lua