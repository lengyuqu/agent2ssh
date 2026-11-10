// Measures the branch K14 could only describe: `OpenProcess(PROCESS_SYNCHRONIZE)`
// failing with ACCESS_DENIED from a restricted token, which is what makes
// `process_handle_is_signaled` return false and the stop stay "cannot verify".
//
// Runs itself twice: the parent keeps its own token, creates a restricted one
// (DISABLE_MAX_PRIVILEGE, no extra SIDs) and launches the child with it. The
// child reports what it can open and what WaitForSingleObject says.
#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, HANDLE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, CreateRestrictedToken, GetTokenInformation, LookupPrivilegeValueW,
        LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, SID_AND_ATTRIBUTES, TOKEN_ADJUST_PRIVILEGES,
        TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessAsUserW, GetCurrentProcess, OpenProcess, OpenProcessToken,
        WaitForSingleObject, CREATE_NO_WINDOW, PROCESS_INFORMATION, PROCESS_SYNCHRONIZE,
        STARTUPINFOW,
    };

    const DISABLE_MAX_PRIVILEGE: u32 = 0x01;

    unsafe fn enable_privilege(token: HANDLE, name: &[u16]) -> u32 {
        let mut luid = Default::default();
        if LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &mut luid) == 0 {
            return GetLastError();
        }
        let mut tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        AdjustTokenPrivileges(token, 0, &mut tp, 0, std::ptr::null_mut(), std::ptr::null_mut());
        GetLastError()
    }

    unsafe fn token_user_sid(token: HANDLE) -> SID_AND_ATTRIBUTES {
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        assert!(
            GetTokenInformation(
                token,
                TokenUser,
                buf.as_mut_ptr() as *mut std::ffi::c_void,
                len,
                &mut len
            ) != 0,
            "GetTokenInformation(TokenUser)"
        );
        // `Sid` points into `buf`; the caller keeps using it after this returns,
        // so the buffer is leaked rather than dropped (one short-lived process).
        let buf = Box::leak(buf.into_boxed_slice());
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        SID_AND_ATTRIBUTES { Sid: user.User.Sid, Attributes: 0 }
    }

    unsafe fn sid(text: &str) -> SID_AND_ATTRIBUTES {
        let wide = text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let mut psid: *mut std::ffi::c_void = std::ptr::null_mut();
        assert!(
            ConvertStringSidToSidW(wide.as_ptr(), &mut psid) != 0,
            "convert {text}"
        );
        SID_AND_ATTRIBUTES { Sid: psid, Attributes: 0 }
    }

    pub fn spawn_restricted(exe: &str, child_args: &str, target: u32, out: &str) -> Result<(), String> {
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ASSIGN_PRIMARY | TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES,
                &mut token,
            ) == 0
            {
                return Err(format!("OpenProcessToken failed: {}", GetLastError()));
            }
            let quota = enable_privilege(token, &wide("SeIncreaseQuotaPrivilege"));
            let assign = enable_privilege(token, &wide("SeAssignPrimaryTokenPrivilege"));
            if quota != 0 && assign != 0 {
                let _ = CloseHandle(token);
                return Err(format!(
                    "cannot enable CreateProcessAsUser privileges (quota={quota}, assign={assign})"
                ));
            }

            // Privileges alone do not make the token unprivileged: the
            // Administrators group is what opens a SYSTEM process, so it has to
            // be deny-only, and restricting SIDs must still cover what the child
            // legitimately needs (Everyone, Users) for its temp file to work.
            let admins = sid("S-1-5-32-544");
            let world = sid("S-1-1-0");
            let users = sid("S-1-5-32-545");
            let me = token_user_sid(token);
            let deny = [admins];
            let restrict = [world, users, me];
            let mut restricted: HANDLE = std::ptr::null_mut();
            if CreateRestrictedToken(
                token,
                DISABLE_MAX_PRIVILEGE,
                deny.len() as u32,
                deny.as_ptr(),
                0,
                std::ptr::null(),
                restrict.len() as u32,
                restrict.as_ptr(),
                &mut restricted,
            ) == 0
            {
                let err = GetLastError();
                let _ = CloseHandle(token);
                return Err(format!("CreateRestrictedToken failed: {err}"));
            }

            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let cmdline = wide(&format!("\"{exe}\" {child_args} {target} \"{out}\""));
            let mut cmdline = cmdline;
            let ok = CreateProcessAsUserW(
                restricted,
                std::ptr::null(),
                cmdline.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                CREATE_NO_WINDOW,
                std::ptr::null(),
                std::ptr::null(),
                &si,
                &mut pi,
            );
            let err = GetLastError();
            let _ = CloseHandle(restricted);
            let _ = CloseHandle(token);
            if ok == 0 {
                return Err(format!("CreateProcessAsUser failed: {err}"));
            }
            let waited = WaitForSingleObject(pi.hProcess, 30_000);
            let _ = CloseHandle(pi.hProcess);
            let _ = CloseHandle(pi.hThread);
            if waited != WAIT_OBJECT_0 {
                return Err(format!("child did not finish, wait={waited}"));
            }
            Ok(())
        }
    }

    pub fn probe(pid: u32) -> String {
        unsafe {
            let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if handle.is_null() {
                let err = GetLastError();
                let kind = if err == ERROR_ACCESS_DENIED {
                    "ACCESS_DENIED (conservative)"
                } else {
                    "other error (conservative)"
                };
                return format!("pid {pid}: OpenProcess=false GetLastError={err} => {kind}");
            }
            let wait = WaitForSingleObject(handle, 0);
            let _ = CloseHandle(handle);
            format!("pid {pid}: OpenProcess=true signaled={}", wait == WAIT_OBJECT_0)
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--child" {
        let pid: u32 = args[2].parse().expect("pid");
        let out = &args[3];
        let mut lines = vec![
            format!("[restricted] {}", win::probe(pid)),
            format!("[restricted] self={}", win::probe(std::process::id())),
        ];
        // stdout of a CreateProcessAsUser child is not wired to our console, so
        // the result travels through a file the parent passes in.
        lines.push(format!("[restricted] done"));
        std::fs::write(out, lines.join("
") + "
").expect("write result file");
        return;
    }

    let targets: Vec<u32> = args.iter().skip(1).filter_map(|a| a.parse().ok()).collect();
    let exe = std::env::current_exe().expect("current exe");
    for pid in targets {
        println!("[elevated ] {}", win::probe(pid));
        let out = std::env::temp_dir().join(format!("a2s-denied-{pid}.txt"));
        let _ = std::fs::remove_file(&out);
        match win::spawn_restricted(&exe.to_string_lossy(), "--child", pid, &out.to_string_lossy()) {
            Ok(_) => match std::fs::read_to_string(&out) {
                Ok(text) => print!("{text}"),
                Err(e) => println!("[restricted] no result file: {e}"),
            },
            Err(e) => println!("[restricted] could not launch: {e}"),
        }
    }

    // The control is in the child branch above: the restricted child opens its
    // own process successfully, which is what rules out a broken token. Here the
    // parent only reports itself, so the two sides can be compared.
    let me = std::process::id();
    println!("[elevated ] {}", win::probe(me));
}
