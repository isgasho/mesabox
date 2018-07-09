use clap::{App, Arg, AppSettings};
use nix::unistd;

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::io::{self, BufRead, Write};
use std::iter;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::result::Result as StdResult;

use ::{UtilData, UtilRead, UtilWrite};
use super::UtilSetup;
use super::ast::ExitCode;
use super::command::{ExecData, InProcessCommand};
use super::env::{EnvFd, Environment};
use super::error::{CmdResult, BuiltinError, CommandError};
use super::option::ShellOption;

type Result<T> = StdResult<T, BuiltinError>;

#[derive(Clone, Debug)]
pub struct BuiltinSet {
    options: Vec<ShellOption>
}

impl BuiltinSet {
    pub fn new(options: Vec<ShellOption>) -> Self {
        Self {
            options: options,
        }
    }

    // XXX: in the future this should check the list of options to figure out what to do
    pub fn find(&self, name: &OsStr) -> Option<Builtin> {
        let name = name.to_string_lossy();
        Some(match &*name {
            "exec" => Builtin::Exec(ExecBuiltin),
            "exit" => Builtin::Exit(ExitBuiltin),
            "export" => Builtin::Export(ExportBuiltin),
            "read" => Builtin::Read(ReadBuiltin),
            "unset" => Builtin::Unset(UnsetBuiltin),
            _ => return None
        })
    }
}

pub enum Builtin {
    Exec(ExecBuiltin),
    Exit(ExitBuiltin),
    Export(ExportBuiltin),
    Read(ReadBuiltin),
    Unset(UnsetBuiltin),
}

impl Builtin {
    fn execute_stdin<I>(&self, env: &mut Environment, data: ExecData, input: &mut I) -> CmdResult<ExitCode>
    where
        I: for<'a> UtilRead<'a>,
    {
        use self::EnvFd::*;

        match env.get_fd(1).current_val().try_clone()? {
            File(mut file) => self.execute_stdout(env, data, input, &mut file),
            Fd(mut fd) => self.execute_stdout(env, data, input, &mut fd),
            // FIXME: this won't work correctly
            Piped(mut piped) => self.execute_stdout(env, data, input, &mut piped),
            Null => self.execute_stdout(env, data, input, &mut io::sink()),
            _ => unimplemented!(),
        }
    }

    fn execute_stdout<I, O>(&self, env: &mut Environment, data: ExecData, input: &mut I, output: &mut O) -> CmdResult<ExitCode>
    where
        I: for<'a> UtilRead<'a>,
        O: for<'a> UtilWrite<'a>,
    {
        use self::EnvFd::*;

        match env.get_fd(2).current_val().try_clone()? {
            File(mut file) => self.execute_stderr(env, data, input, output, &mut file),
            Fd(mut fd) => self.execute_stderr(env, data, input, output, &mut fd),
            // FIXME: this won't work correctly
            Piped(mut piped) => self.execute_stderr(env, data, input, output, &mut piped),
            Null => self.execute_stderr(env, data, input, output, &mut io::sink()),
            _ => unimplemented!(),
        }
    }

    fn execute_stderr<I, O, E>(&self, env: &mut Environment, data: ExecData, input: &mut I, output: &mut O, error: &mut E) -> CmdResult<ExitCode>
    where
        I: for<'a> UtilRead<'a>,
        O: for<'a> UtilWrite<'a>,
        E: for<'a> UtilWrite<'a>,
    {
        use self::Builtin::*;

        // TODO: we let the current_dir be empty because that should be set in Environment most likely
        let mut util_setup = UtilData::new(input, output, error, iter::empty(), None);
        let setup = &mut util_setup;

        match self {
            Exec(u) => u.run(setup, env, data),
            Exit(u) => u.run(setup, env, data),
            Export(u) => u.run(setup, env, data),
            Read(u) => u.run(setup, env, data),
            Unset(u) => u.run(setup, env, data),
        }.map_err(|e| CommandError::Builtin(e))
    }
}

impl InProcessCommand for Builtin {
    fn execute<S: UtilSetup>(&self, setup: &mut S, env: &mut Environment, data: ExecData) -> CmdResult<ExitCode> {
        use self::EnvFd::*;

        let res = match env.get_fd(0).current_val().try_clone()? {
            File(mut file) => self.execute_stdin(env, data, &mut file),
            Fd(mut fd) => self.execute_stdin(env, data, &mut fd),
            Piped(piped) => self.execute_stdin(env, data, &mut &piped[..]),
            Null => self.execute_stdin(env, data, &mut io::empty()),
            _ => unimplemented!(),
        };

        Ok(match res {
            Ok(m) => m,
            Err(f) => {
                // XXX: do we really want to ignore write errors?
                // FIXME: should probably not write to setup.error() unless we create a new
                //        UtilData struct each time we call a builtin
                let _ = writeln!(setup.error(), "{}", f);
                1
            }
        })
    }
}

trait BuiltinSetup {
    fn run<S: UtilSetup>(&self, setup: &mut S, env: &mut Environment, data: ExecData) -> Result<ExitCode>;
}

pub struct ExecBuiltin;

// XXX: given that this replaces the current process, if we are being used as a library the calling
//      process will be replaced.  this could be an issue when e.g. running our tests
// TODO: because this needs to affect the "current shell execution environment," we need to somehow
//       return the fds to the parent environment
impl BuiltinSetup for ExecBuiltin {
    fn run<S>(&self, setup: &mut S, env: &mut Environment, data: ExecData) -> Result<ExitCode>
    where
        S: UtilSetup,
    {
        use std::process::{Command, Stdio};
        use std::os::unix::io::FromRawFd;
        use std::os::unix::process::CommandExt;

        let mut args = data.args.into_iter();
        if let Some(name) = args.next() {
            // replace the current process with that started by the given command
            let mut cmd = Command::new(name);
            cmd.args(args)
                .env_clear()
                .envs(env.export_iter())
                .envs(data.env.iter());

            // TODO: figure out what to do if one of the IO interfaces doesn't have a file
            //       descriptor (such as as Vec<u8>).  afaict this is only really an issue with
            //       heredocs and when we are called as a library from a process that most likely
            //       does not actually want to be replaced
            // NOTE: we need to duplicate the fds as from_raw_fd() takes ownership
            // TODO: this needs to duplicate all the fds (3-9 because stdin/stdout/stderr are done
            //       already below) like in command.rs
            if let Some(fd) = setup.input().raw_fd() {
                let fd = unistd::dup(fd)?;
                cmd.stdin(unsafe { Stdio::from_raw_fd(fd) });
            }
            if let Some(fd) = setup.output().raw_fd() {
                let fd = unistd::dup(fd)?;
                cmd.stdout(unsafe { Stdio::from_raw_fd(fd) });
            }
            if let Some(fd) = setup.error().raw_fd() {
                let fd = unistd::dup(fd)?;
                cmd.stderr(unsafe { Stdio::from_raw_fd(fd) });
            }

            // if this actually returns an error the process failed to start
            Err(cmd.exec().into())
        } else {
            Ok(0)
        }
    }
}

pub struct ExitBuiltin;

impl BuiltinSetup for ExitBuiltin {
    fn run<S: UtilSetup>(&self, _setup: &mut S, _env: &mut Environment, _data: ExecData) -> Result<ExitCode> {
        // TODO: figure out how to exit properly
        unimplemented!()
    }
}

pub struct ExportBuiltin;

impl BuiltinSetup for ExportBuiltin {
    // TODO: needs to support -p option
    fn run<S>(&self, _setup: &mut S, env: &mut Environment, data: ExecData) -> Result<ExitCode>
    where
        S: UtilSetup,
    {
        // TODO: need to split args like VarAssign (we are just assuming names are given atm)
        for arg in data.args {
            env.export_var(Cow::Owned(arg));
        }

        Ok(0)
    }
}

pub struct UnsetBuiltin;

impl BuiltinSetup for UnsetBuiltin {
    fn run<S>(&self, _setup: &mut S, env: &mut Environment, data: ExecData) -> Result<ExitCode>
    where
        S: UtilSetup,
    {
        // TODO: suppress --help/--version (non-POSIX, although they could perhaps serve as an extension)
        let matches = App::new("unset")
            .setting(AppSettings::NoBinaryName)
            .arg(Arg::with_name("function")
                .short("f")
                .overrides_with("variable"))
            .arg(Arg::with_name("variable")
                .short("v"))
            .arg(Arg::with_name("NAMES")
                .index(1)
                .multiple(true))
            .get_matches_from_safe(data.args)?;

        let func = matches.is_present("function");

        // TODO: if variable/whatever is readonly, this function should return >0 and NOT remove that
        //       variable
        if let Some(values) = matches.values_of_os("NAMES") {
            for name in values {
                if func {
                    env.remove_func(name);
                } else {
                    env.remove_var(name);
                }
            }
        }

        Ok(0)
    }
}

pub struct ReadBuiltin;

impl BuiltinSetup for ReadBuiltin {
    fn run<S>(&self, setup: &mut S, env: &mut Environment, data: ExecData) -> Result<ExitCode>
    where
        S: UtilSetup,
    {
        let matches = App::new("read")
            .setting(AppSettings::NoBinaryName)
            // if present we treat backslash as a normal character rather than the start of an escape
            // sequence
            .arg(Arg::with_name("backslash")
                .short("r"))
            .arg(Arg::with_name("VARS")
                .index(1)
                .multiple(true)
                .required(true))
            .get_matches_from_safe(data.args)?;

        let input = setup.input();
        let mut input = input.lock_reader()?;

        let ignore_backslash = matches.is_present("backslash");

        let check_backslash = |buffer: &mut Vec<u8>| {
            loop {
                let res = match buffer.iter().last() {
                    Some(b'\n') => {
                        buffer.pop();
                        continue;
                    }
                    Some(b'\\') => {
                        // need to make sure this byte isn't escaped
                        buffer.iter().rev().skip(1).take_while(|&&byte| byte == b'\\').count() % 2 == 1
                    }
                    _ => true,
                };
                return res;
            }
        };

        let mut buffer = vec![];
        loop {
            // TODO: check for EOF
            input.read_until(b'\n', &mut buffer)?;
            let not_backslash = check_backslash(&mut buffer);
            // TODO: handle heredoc portion?
            if ignore_backslash || not_backslash {
                break;
            }
            // we need to remove the backslash
            buffer.pop();
        }

        let vars = matches.values_of_os("VARS").unwrap();
        let var_count = vars.clone().count();

        let field_iter = {
            // XXX: maybe this should be extracted into a separate function (i feel like this will be used
            //      to split fields normally too)
            let ifs = env.get_var("IFS").map(|v| v.clone()).unwrap_or_else(|| OsString::from(" \t\n"));
            buffer.splitn(var_count, move |byte| {
                ifs.as_bytes().contains(byte)
            })
        };
        for (var, value) in vars.zip(field_iter) {
            let value = if ignore_backslash {
                value.to_owned()
            } else {
                let mut result = Vec::with_capacity(value.len());
                let mut in_escape = false;
                for &byte in value {
                    if in_escape {
                        result.push(byte);
                        in_escape = false;
                    } else {
                        if byte == b'\\' {
                            in_escape = true;
                        } else {
                            result.push(byte);
                        }
                    }
                }
                // it should be impossible for there to be an extra escape
                result
            };
            env.set_var(Cow::Borrowed(var), OsString::from_vec(value));
        }

        Ok(0)
    }
}
