//
// Copyright (c) 2018, The MesaLock Linux Project Contributors
// All rights reserved.
//
// This work is licensed under the terms of the BSD 3-Clause License.
// For a copy, see the LICENSE file.
//

macro_rules! util_app {
    ($name:expr) => {
        util_app!($name, self::DESCRIPTION)
    };
    ($name:expr, $desc:expr) => {{
        ::clap::App::new($name)
            .version(crate_version!())
            .author(crate_authors!())
            .about($desc)
    }};
}

// FIXME: should use name given on the command-line rather than a hard-coded one
macro_rules! display_msg {
    ($stream:expr, $($args:tt)+) => {
        writeln!($stream, "{}: {}", self::NAME, format_args!($($args)+))
    }
}

macro_rules! display_err {
    ($stream:expr, $($args:tt)+) => {
        display_msg!($stream, "error: {}", format_args!($($args)+))
    }
}

macro_rules! display_warn {
    ($stream:expr, $($args:tt)+) => {
        display_msg!($stream, "warning: {}", format_args!($($args)+))
    }
}
