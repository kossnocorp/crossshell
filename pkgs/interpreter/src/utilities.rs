use std::{ffi::OsString, path::Path};

macro_rules! utilities {
    ($($name:literal => $implementation:ident),+ $(,)?) => {
        /// Bundled external commands. Shell builtins take precedence.
        pub const UTILITIES: &[&str] = &[$($name),+];

        fn dispatch(name: &str, args: Vec<OsString>) -> i32 {
            match name {
                $($name => $implementation::uumain(args.into_iter()),)+
                _ => unreachable!("only registered utilities are dispatched"),
            }
        }
    };
}

utilities! {
    "base64" => uu_base64, "basename" => uu_basename, "cat" => uu_cat,
    "cp" => uu_cp, "cut" => uu_cut, "date" => uu_date,
    "dirname" => uu_dirname, "echo" => uu_echo, "env" => uu_env,
    "false" => uu_false, "head" => uu_head, "ln" => uu_ln,
    "ls" => uu_ls, "mkdir" => uu_mkdir, "mktemp" => uu_mktemp,
    "mv" => uu_mv, "paste" => uu_paste, "printenv" => uu_printenv,
    "printf" => uu_printf, "pwd" => uu_pwd, "readlink" => uu_readlink,
    "realpath" => uu_realpath, "rm" => uu_rm, "rmdir" => uu_rmdir,
    "seq" => uu_seq, "sleep" => uu_sleep, "sort" => uu_sort,
    "split" => uu_split, "tail" => uu_tail, "tee" => uu_tee,
    "test" => uu_test, "[" => uu_test, "touch" => uu_touch,
    "tr" => uu_tr, "true" => uu_true, "truncate" => uu_truncate,
    "uname" => uu_uname, "uniq" => uu_uniq, "wc" => uu_wc, "yes" => uu_yes,
}

/// Dispatch a multicall utility invocation, returning `None` for a normal host
/// invocation. Call this at the very start of the embedding executable's `main`,
/// before creating threads or parsing its own arguments, and exit with the code
/// if it returns `Some`. uutils uses process-global state and native stdio.
pub fn dispatch_utility() -> Option<i32> {
    let mut args: Vec<_> = std::env::args_os().collect();
    let path = Path::new(args.first()?);
    let name = path.file_name()?.to_str()?;
    #[cfg(windows)]
    let name = name.strip_suffix(".exe").unwrap_or(name);
    if !UTILITIES.contains(&name) {
        return None;
    }
    let name = name.to_owned();
    args[0] = OsString::from(&name);
    let locale_name = if name == "[" { "test" } else { &name };
    if let Err(error) = uucore::locale::setup_localization(locale_name) {
        eprintln!("{name}: {error}");
        return Some(1);
    }
    Some(dispatch(&name, args))
}
