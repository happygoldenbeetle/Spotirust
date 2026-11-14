use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString, c_void},
    mem::{size_of, zeroed},
    os::windows::ffi::{OsStrExt, OsStringExt},
    ptr::{null, null_mut},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{Diagnostics::Debug::*, JobObjects::*, Memory::*, ProcessStatus::*, Threading::*},
    UI::WindowsAndMessaging::*,
};

const MODULE_HASH: &str = "769ac5a1a282f470b663a1c3933774f56d954f8f0dbfd156f9d799a173c79406";
const STORAGE_VTABLE: usize = 0x1ba8dd8;
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
fn plain(path: &Path) -> PathBuf {
    let text: Vec<_> = path.as_os_str().encode_wide().collect();
    if text.starts_with(&[92, 92, 63, 92]) {
        if text[4..].starts_with(&[85, 78, 67, 92]) {
            PathBuf::from(OsString::from_wide(&[&[92, 92], &text[8..]].concat()))
        } else {
            PathBuf::from(OsString::from_wide(&text[4..]))
        }
    } else {
        path.to_path_buf()
    }
}
fn check(ok: i32, label: &str) -> Result<()> {
    if ok == 0 {
        Err(std::io::Error::last_os_error()).context(label.to_string())
    } else {
        Ok(())
    }
}
fn verify_module(spotify: &Path) -> Result<()> {
    let hash = encode(&Sha256::digest(fs::read(spotify.join("Spotify.dll"))?));
    anyhow::ensure!(
        hash == MODULE_HASH,
        "installed Spotify version is not supported by this storage initializer"
    );
    Ok(())
}

fn low_directory(path: &Path) -> Result<()> {
    unsafe {
        let mut descriptor = null_mut();
        check(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide("S:(ML;OICI;NW;;;LW)").as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            ),
            "create low integrity descriptor",
        )?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let created = CreateDirectoryW(wide(path).as_ptr(), &attributes);
        let error = std::io::Error::last_os_error();
        LocalFree(descriptor);
        if created == 0 {
            return Err(error).context("create isolated cache initializer directory");
        }
    }
    Ok(())
}

pub(super) fn capture(
    snapshot: &Path,
    header: &[u8; INDEX_HEADER_SIZE],
    parent: &Path,
) -> Result<State> {
    let spotify =
        PathBuf::from(std::env::var_os("APPDATA").context("APPDATA unavailable")?).join("Spotify");
    verify_module(&spotify)?;
    let parent = parent.canonicalize()?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let bootstrap =
        plain(&parent.join(format!("cache-initializer-{}-{stamp}", std::process::id())));
    low_directory(&bootstrap)?;
    for path in [
        "cache/Storage",
        "cache/Data",
        "roaming/Spotify",
        "local",
        "temporary",
    ] {
        fs::create_dir_all(bootstrap.join(path))?;
    }
    let cache = bootstrap.join("cache");
    fs::write(cache.join("Storage/index.dat"), header)?;
    fs::write(
        bootstrap.join("roaming/Spotify/prefs"),
        format!(
            "storage.last-location={}\n",
            serde_json::to_string(&cache.join("Storage"))?
        ),
    )?;
    unsafe {
        let mut original = null_mut();
        check(
            OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut original),
            "open initializer token",
        )?;
        let original = Handle(original);
        let mut restricted = null_mut();
        check(
            CreateRestrictedToken(
                original.0,
                DISABLE_MAX_PRIVILEGE,
                0,
                null(),
                0,
                null(),
                0,
                null(),
                &mut restricted,
            ),
            "restrict initializer token",
        )?;
        let restricted = Handle(restricted);
        let mut sid = null_mut();
        check(
            ConvertStringSidToSidW(wide("S-1-16-4096").as_ptr(), &mut sid),
            "create low integrity SID",
        )?;
        let label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: sid,
                Attributes: 0x20,
            },
        };
        let set = SetTokenInformation(
            restricted.0,
            TokenIntegrityLevel,
            &label as *const _ as *const c_void,
            (size_of::<TOKEN_MANDATORY_LABEL>() as u32) + GetLengthSid(sid),
        );
        let error = std::io::Error::last_os_error();
        LocalFree(sid);
        if set == 0 {
            return Err(error).context("apply low integrity token");
        }
        let job = Handle(CreateJobObjectW(null(), null()));
        anyhow::ensure!(!job.0.is_null(), "create initializer process job failed");
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = 32;
        check(
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const c_void,
                size_of_val(&limits) as u32,
            ),
            "limit initializer process job",
        )?;
        let executable = std::env::current_exe()?;
        let args = [
            executable.as_os_str(),
            OsStr::new("--cache-init-helper"),
            bootstrap.as_os_str(),
            snapshot.as_os_str(),
            spotify.as_os_str(),
        ];
        let command = args
            .iter()
            .map(|arg| quote(arg))
            .collect::<Vec<_>>()
            .join(" ");
        let mut command = wide(command);
        let mut startup: STARTUPINFOW = zeroed();
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        startup.dwFlags = STARTF_USESHOWWINDOW;
        startup.wShowWindow = SW_HIDE as u16;
        let mut process: PROCESS_INFORMATION = zeroed();
        check(
            CreateProcessAsUserW(
                restricted.0,
                null(),
                command.as_mut_ptr(),
                null(),
                null(),
                0,
                CREATE_SUSPENDED | CREATE_NO_WINDOW,
                null(),
                wide(&bootstrap).as_ptr(),
                &startup,
                &mut process,
            ),
            "start restricted initializer helper",
        )?;
        let main = Handle(process.hProcess);
        let thread = Handle(process.hThread);
        if AssignProcessToJobObject(job.0, main.0) == 0 {
            TerminateProcess(main.0, 1);
            anyhow::bail!("assign initializer process job failed");
        }
        if ResumeThread(thread.0) == u32::MAX {
            TerminateProcess(main.0, 1);
            anyhow::bail!("resume initializer helper failed");
        }
        if WaitForSingleObject(main.0, 30_000) != WAIT_OBJECT_0 {
            drop(job);
            anyhow::bail!("isolated storage initialization timed out");
        }
        // Closing this job stops only this helper and its newly created children.
        drop(job);
    }
    let result = bootstrap.join("state.json");
    let state = fs::read(&result)
        .context("isolated initializer did not produce validated storage state")?;
    let state: State = serde_json::from_slice(&state).context("invalid isolated storage state")?;
    // The small isolated profile is retained for diagnostics. It contains no
    // source audio or login credentials, and no recursive deletion is performed.
    Ok(state)
}

fn quote(value: &OsStr) -> String {
    let value = value.to_string_lossy();
    let mut result = String::from("\"");
    let mut slashes = 0;
    for character in value.chars() {
        if character == '\\' {
            slashes += 1;
            continue;
        }
        if character == '"' {
            result.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
        } else {
            result.extend(std::iter::repeat_n('\\', slashes));
        }
        result.push(character);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}

fn integrity() -> Result<u32> {
    unsafe {
        let mut token = null_mut();
        check(
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token),
            "query helper token",
        )?;
        let token = Handle(token);
        let mut needed = 0;
        GetTokenInformation(token.0, TokenIntegrityLevel, null_mut(), 0, &mut needed);
        let mut data = vec![0_u8; needed as usize];
        check(
            GetTokenInformation(
                token.0,
                TokenIntegrityLevel,
                data.as_mut_ptr().cast(),
                needed,
                &mut needed,
            ),
            "read helper integrity",
        )?;
        let label = &*(data.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        Ok(*GetSidSubAuthority(label.Label.Sid, u32::from(count - 1)))
    }
}

unsafe extern "system" fn park(hwnd: HWND, pid: LPARAM) -> i32 {
    unsafe {
        let mut owner = 0;
        GetWindowThreadProcessId(hwnd, &mut owner);
        if owner == pid as u32 {
            SetWindowPos(
                hwnd,
                null_mut(),
                -30000,
                -30000,
                1200,
                900,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }
    1
}

pub(super) fn helper(bootstrap: &Path, snapshot: &Path, spotify: &Path) -> Result<()> {
    anyhow::ensure!(
        integrity()? <= 4096,
        "cache initializer helper must run at low integrity"
    );
    verify_module(spotify)?;
    let original =
        PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA unavailable")?)
            .join("Spotify");
    for relative in [
        "",
        "Data",
        "Storage",
        "Browser/Cache",
        "public.ldb",
        "Users",
    ] {
        let path = original.join(relative);
        if !path.exists() {
            continue;
        }
        unsafe {
            let handle = CreateFileW(
                wide(&path).as_ptr(),
                GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                null_mut(),
            );
            let error = GetLastError();
            if handle != INVALID_HANDLE_VALUE {
                CloseHandle(handle);
                anyhow::bail!("original cache write isolation failed");
            }
            anyhow::ensure!(
                error == ERROR_ACCESS_DENIED,
                "original cache isolation could not be established"
            );
        }
    }
    let cache = bootstrap.join("cache");
    let (child, pid) = launch_reference(bootstrap, spotify, &cache)?;
    let index = fs::read(snapshot.join("Storage/index.dat"))?;
    anyhow::ensure!(
        index.len() >= INDEX_HEADER_SIZE + INDEX_RECORD_SIZE,
        "copied index has no entries to verify"
    );
    let start = Instant::now();
    loop {
        unsafe {
            EnumWindows(Some(park), pid as LPARAM);
        }
        let mut code = 0;
        unsafe {
            check(
                GetExitCodeProcess(child.0, &mut code),
                "query isolated Spotify process",
            )?;
        }
        anyhow::ensure!(
            code == STILL_ACTIVE as u32,
            "isolated Spotify exited before initialization; close other Spotify instances and retry"
        );
        if let Some(state) = scan(pid, &index)? {
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(bootstrap.join("state.json"))?;
            output.write_all(&serde_json::to_vec(&state)?)?;
            output.sync_all()?;
            return Ok(());
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(20),
            "compatible native storage state was not found"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn launch_reference(bootstrap: &Path, spotify: &Path, cache: &Path) -> Result<(Handle, u32)> {
    let arguments = [
        spotify.join("Spotify.exe").into_os_string(),
        format!("--cache-path={}", cache.display()).into(),
        format!("--user-data-dir={}", cache.join("Browser").display()).into(),
        "--minimized".into(),
        "--disable-gpu".into(),
        "--mute-audio".into(),
        "--no-default-browser-check".into(),
        "--no-sandbox".into(),
    ];
    let mut command = wide(
        arguments
            .iter()
            .map(|arg| quote(arg))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let mut variables: BTreeMap<_, _> = std::env::vars_os()
        .map(|(key, value)| (key.to_string_lossy().to_ascii_uppercase(), value))
        .collect();
    for (key, value) in [
        ("APPDATA", bootstrap.join("roaming").into_os_string()),
        ("LOCALAPPDATA", bootstrap.join("local").into_os_string()),
        ("TEMP", bootstrap.join("temporary").into_os_string()),
        ("TMP", bootstrap.join("temporary").into_os_string()),
        ("__COMPAT_LAYER", "RunAsInvoker".into()),
    ] {
        variables.insert(key.into(), value);
    }
    let mut environment = Vec::new();
    for (key, value) in variables {
        environment.extend(key.encode_utf16());
        environment.push(61);
        environment.extend(value.encode_wide());
        environment.push(0);
    }
    environment.push(0);
    unsafe {
        let mut startup: STARTUPINFOW = zeroed();
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        startup.dwFlags = STARTF_USESHOWWINDOW;
        startup.wShowWindow = SW_HIDE as u16;
        let mut process: PROCESS_INFORMATION = zeroed();
        check(
            CreateProcessW(
                null(),
                command.as_mut_ptr(),
                null(),
                null(),
                0,
                CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                wide(bootstrap).as_ptr(),
                &startup,
                &mut process,
            ),
            "launch isolated Spotify",
        )?;
        let _thread = Handle(process.hThread);
        Ok((Handle(process.hProcess), process.dwProcessId))
    }
}

fn read(process: HANDLE, address: usize, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    let mut count = 0;
    if unsafe {
        ReadProcessMemory(
            process,
            address as *const c_void,
            bytes.as_mut_ptr().cast(),
            size,
            &mut count,
        )
    } == 0
    {
        return Vec::new();
    }
    bytes.truncate(count);
    bytes
}
fn word(bytes: &[u8], offset: usize) -> Option<usize> {
    Some(u64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?) as usize)
}
fn scan(pid: u32, index: &[u8]) -> Result<Option<State>> {
    unsafe {
        let process = Handle(OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            0,
            pid,
        ));
        if process.0.is_null() {
            return Ok(None);
        }
        let mut modules = [null_mut(); 512];
        let mut needed = 0;
        if K32EnumProcessModules(
            process.0,
            modules.as_mut_ptr(),
            size_of_val(&modules) as u32,
            &mut needed,
        ) == 0
        {
            return Ok(None);
        }
        let mut base = None;
        for module in modules
            .iter()
            .take((needed as usize / size_of::<HMODULE>()).min(modules.len()))
        {
            let mut name = [0_u16; 260];
            let length =
                K32GetModuleBaseNameW(process.0, *module, name.as_mut_ptr(), name.len() as u32);
            if String::from_utf16_lossy(&name[..length as usize])
                .eq_ignore_ascii_case("spotify.dll")
            {
                base = Some(*module as usize);
                break;
            }
        }
        let Some(base) = base else { return Ok(None) };
        let pattern = (base + STORAGE_VTABLE).to_le_bytes();
        let mut address = 0;
        let started = Instant::now();
        while address < 0x7fffffffffff && started.elapsed() < Duration::from_secs(3) {
            let mut region: MEMORY_BASIC_INFORMATION = zeroed();
            if VirtualQueryEx(
                process.0,
                address as *const c_void,
                &mut region,
                size_of_val(&region),
            ) == 0
            {
                break;
            }
            let start = region.BaseAddress as usize;
            let size = region.RegionSize;
            if size == 0 {
                break;
            }
            address = start.saturating_add(size);
            if region.State != MEM_COMMIT
                || region.Type != MEM_PRIVATE
                || region.Protect & PAGE_GUARD != 0
                || !matches!(
                    region.Protect & 0xff,
                    PAGE_READWRITE
                        | PAGE_WRITECOPY
                        | PAGE_EXECUTE_READWRITE
                        | PAGE_EXECUTE_WRITECOPY
                )
            {
                continue;
            }
            for offset in (0..size).step_by(1024 * 1024) {
                let bytes = read(process.0, start + offset, (1024 * 1024).min(size - offset));
                for (position, _) in bytes
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .enumerate()
                    .filter(|(_, window)| **window == pattern)
                {
                    let state = read(process.0, start + offset + position * 8, 0x350);
                    let Some(cipher) = word(&state, 0x338) else {
                        continue;
                    };
                    let Some(schedule) = word(&read(process.0, cipher, 16), 8) else {
                        continue;
                    };
                    let schedule = read(process.0, schedule, 16);
                    if schedule.len() != 16 || state.len() < 0x334 {
                        continue;
                    }
                    let mut key = [0; 16];
                    for (out, input) in key
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .zip(schedule.as_chunks::<4>().0.iter())
                    {
                        for (out, input) in out.iter_mut().zip(input.iter().rev()) {
                            *out = *input;
                        }
                    }
                    if !candidate(&key, index) {
                        continue;
                    }
                    return Ok(Some(State {
                        storage_key_hex: encode(&key),
                        naming_state_hex: encode(&state[0x320..0x334]),
                        configured_override: word(&state, 0xf8).unwrap_or_default() as u64,
                    }));
                }
            }
        }
    }
    Ok(None)
}
