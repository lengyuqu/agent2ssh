// What does `daemon status` say when the OS refuses to describe the recorded
// PID, and does `daemon stop` still refuse to signal it?
//
// `OpenProcess` refusing a *daemon this account started* needs two things at
// once: a token that lacks the process DACL's grants (a restricted token with
// Administrators deny-only) and a process whose DACL does not name the user.
// The parent builds both: it starts a stand-in named `agent2ssh-daemon.exe`,
// rewrites that process's DACL to SYSTEM + Administrators only, then launches
// the CLI through a restricted token.
//
// Everything is synthetic: a throwaway config dir under %TEMP%, a copy of
// ping.exe as the stand-in, no real daemon and no real credential.
#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, LocalFree, ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::Authorization::{
        SetEntriesInAclW, SetSecurityInfo, ConvertStringSidToSidW, EXPLICIT_ACCESS_W,
        NO_MULTIPLE_TRUSTEE, SE_KERNEL_OBJECT, SET_ACCESS, TRUSTEE_IS_SID,
        TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, CreateRestrictedToken, GetTokenInformation, LookupPrivilegeValueW,
        DACL_SECURITY_INFORMATION, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        SID_AND_ATTRIBUTES, TOKEN_ADJUST_PRIVILEGES, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE,
        TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessAsUserW, GetCurrentProcess, OpenProcess, OpenProcessToken,
        WaitForSingleObject, CREATE_NO_WINDOW, PROCESS_ALL_ACCESS, PROCESS_INFORMATION,
        STARTUPINFOW,
    };

    // DISABLE_MAX_PRIVILEGE (0x01) also removes SeChangeNotifyPrivilege, and a
    // process without it cannot traverse directories to load its DLLs
    // (STATUS_DLL_INIT_FAILED, 0xC0000142). Keep the privileges but make
    // Administrators deny-only: DACL checks then refuse, which is the point.
    const RESTRICTED_FLAGS: u32 = 0;
    const PROCESS_ALL: u32 = 0x1F_FFFF;

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
        AdjustTokenPrivileges(
            token,
            0,
            &mut tp,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
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
        let buf = Box::leak(buf.into_boxed_slice());
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        SID_AND_ATTRIBUTES {
            Sid: user.User.Sid,
            Attributes: 0,
        }
    }

    unsafe fn sid(text: &str) -> SID_AND_ATTRIBUTES {
        let wide = text
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>();
        let mut psid: *mut std::ffi::c_void = std::ptr::null_mut();
        assert!(
            ConvertStringSidToSidW(wide.as_ptr(), &mut psid) != 0,
            "convert {text}"
        );
        SID_AND_ATTRIBUTES {
            Sid: psid,
            Attributes: 0,
        }
    }

    /// Rewrite the process DACL so only SYSTEM and Administrators are granted.
    ///
    /// A restricted token keeps the user SID but makes Administrators deny-only,
    /// so this is what makes `OpenProcess` answer ACCESS_DENIED for a process the
    /// same account started.
    pub fn restrict_process_to_admins_only(pid: u32) -> Result<(), String> {
        unsafe {
            let handle = OpenProcess(PROCESS_ALL_ACCESS, 0, pid);
            if handle.is_null() {
                return Err(format!(
                    "OpenProcess(all) for DACL rewrite failed: {}",
                    GetLastError()
                ));
            }
            let system = sid("S-1-5-18");
            let admins = sid("S-1-5-32-544");
            let entries = [
                explicit(system, "SYSTEM"),
                explicit(admins, "Administrators"),
            ];
            let mut acl: *mut windows_sys::Win32::Security::ACL = std::ptr::null_mut();
            let rc = SetEntriesInAclW(
                entries.len() as u32,
                entries.as_ptr(),
                std::ptr::null_mut(),
                &mut acl,
            );
            if rc != ERROR_SUCCESS {
                CloseHandle(handle);
                return Err(format!("SetEntriesInAclW failed: {rc}"));
            }
            let rc = SetSecurityInfo(
                handle,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                acl,
                std::ptr::null_mut(),
            );
            let _ = LocalFree(acl as *mut std::ffi::c_void);
            CloseHandle(handle);
            if rc != ERROR_SUCCESS {
                return Err(format!("SetSecurityInfo failed: {rc}"));
            }
            Ok(())
        }
    }

    fn explicit(sid: SID_AND_ATTRIBUTES, _label: &str) -> EXPLICIT_ACCESS_W {
        EXPLICIT_ACCESS_W {
            grfAccessPermissions: PROCESS_ALL,
            grfAccessMode: SET_ACCESS,
            grfInheritance: 0,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: std::ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
                ptstrName: sid.Sid as *mut u16,
            },
        }
    }

    pub fn spawn_restricted(exe: &str, child_args: &str, out: &str) -> Result<(), String> {
        let wide = |s: &str| {
            s.encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
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
            let admins = sid("S-1-5-32-544");
            let world = sid("S-1-1-0");
            let users = sid("S-1-5-32-545");
            // `INTERACTIVE` has to stay in the restricted set: without it the
            // child cannot open the window station / desktop it inherits, and
            // every process it starts dies with STATUS_DLL_INIT_FAILED
            // (0xC0000142) — measured with `whoami.exe` as the control.
            let interactive = sid("S-1-5-4");
            let me = token_user_sid(token);
            let deny = [admins];
            let restrict = [world, users, interactive, me];
            let mut restricted: HANDLE = std::ptr::null_mut();
            if CreateRestrictedToken(
                token,
                RESTRICTED_FLAGS,
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
            // `CreateProcessAsUser` under a restricted token needs the desktop
            // named explicitly; leaving it NULL makes every child (even
            // `whoami.exe`) die with STATUS_DLL_INIT_FAILED.
            let mut desktop = wide("winsta0\\default");
            si.lpDesktop = desktop.as_mut_ptr();
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let mut cmdline = wide(&format!("\"{exe}\" {child_args} \"{out}\""));
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
            let waited = WaitForSingleObject(pi.hProcess, 60_000);
            let _ = CloseHandle(pi.hProcess);
            let _ = CloseHandle(pi.hThread);
            if waited != WAIT_OBJECT_0 {
                return Err(format!("child did not finish, wait={waited}"));
            }
            Ok(())
        }
    }

    pub fn can_open(pid: u32) -> String {
        unsafe {
            let handle = OpenProcess(0x0010_0000 | 0x0000_1000, 0, pid); // SYNCHRONIZE | QUERY_LIMITED
            if handle.is_null() {
                let err = GetLastError();
                format!("OpenProcess=false GetLastError={err}")
            } else {
                let wait = WaitForSingleObject(handle, 0);
                CloseHandle(handle);
                format!("OpenProcess=true signaled={}", wait == WAIT_OBJECT_0)
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--child" {
        // argv: --child <cli> <out>; environment carries the config dir and address.
        let cli = &args[2];
        let out = &args[3];
        let run = |extra: &[&str]| -> String {
            use std::os::windows::process::CommandExt;
            // A restricted token cannot always get a console for a child, which
            // shows up as STATUS_DLL_INIT_FAILED (0xC0000142); ask for no window
            // and let stdout/stderr be the pipes `output()` already creates.
            match std::process::Command::new(cli)
                .args(extra)
                .creation_flags(0x0800_0000)
                .output()
            {
                Ok(output) => format!(
                    "{{\"argv\":{:?},\"status\":{},\"stdout\":{:?},\"stderr\":{:?}}}",
                    extra,
                    output.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ),
                Err(error) => format!("{{\"argv\":{extra:?},\"error\":\"{error}\"}}"),
            }
        };
        let mut results = Vec::new();
        // Control: can this restricted token start anything at all? If this
        // works and the CLI does not, the token is fine and the CLI's own
        // dependencies are the difference.
        match std::process::Command::new(r"C:\Windows\System32\whoami.exe").output() {
            Ok(output) => results.push(format!(
                "{{\"control\":\"whoami\",\"status\":{},\"stdout\":{:?}}}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stdout)
            )),
            Err(error) => results.push(format!("{{\"control\":\"whoami\",\"error\":\"{error}\"}}")),
        }
        results.push(run(&["daemon", "status", "--json"]));
        results.push(run(&["daemon", "stop"]));
        results.push(format!(
            "{{\"open_self\":\"{}\"}}",
            win::can_open(std::process::id())
        ));
        std::fs::write(out, results.join("\n")).expect("write child results");
        return;
    }

    // Parent: argv[1] is the CLI under test, argv[2] is the stand-in PID to expect.
    let cli = args.get(1).cloned().expect("usage: probe <agent2ssh.exe>");
    let pid: u32 = args
        .get(2)
        .and_then(|value| value.parse().ok())
        .expect("usage: probe <agent2ssh.exe> <stand-in-pid>");

    println!("[elevated ] can_open={}", win::can_open(pid));
    match win::restrict_process_to_admins_only(pid) {
        Ok(()) => println!("[elevated ] DACL rewritten to SYSTEM + Administrators"),
        Err(error) => {
            println!("[elevated ] DACL rewrite failed: {error}");
            return;
        }
    }
    println!("[elevated ] can_open_after={}", win::can_open(pid));

    let out = std::env::temp_dir().join("a2s-denied-child.txt");
    let _ = std::fs::remove_file(&out);
    let exe = std::env::current_exe().expect("current exe");
    match win::spawn_restricted(
        &exe.to_string_lossy(),
        &format!("--child \"{cli}\""),
        &out.to_string_lossy(),
    ) {
        Ok(()) => match std::fs::read_to_string(&out) {
            Ok(text) => print!("{text}"),
            Err(error) => println!("[restricted] no result file: {error}"),
        },
        Err(error) => println!("[restricted] could not launch: {error}"),
    }
}
