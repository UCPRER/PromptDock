use crate::data::{Action, SendKey};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Target {
    pub hwnd: String,
    pub pid: u32,
    pub title: String,
    pub process: String,
    pub bound: bool,
}
#[derive(Clone, Debug)]
pub struct Binding {
    pub runtime_id: Vec<i32>,
}
pub struct ExecutionResult {
    pub message: String,
    pub binding: Option<Binding>,
}

#[cfg(windows)]
mod win {
    use super::*;
    use crate::input::{self, Candidate, InputKind};
    use std::{
        ffi::c_void,
        mem::size_of,
        path::Path,
        thread,
        time::{Duration, Instant},
    };
    use windows::core::Interface;
    use windows::Win32::{
        Foundation::HWND,
        System::{
            Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
            Variant::VARIANT,
        },
        UI::Accessibility::*,
    };

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct KeyboardInput {
        vk: u16,
        scan: u16,
        flags: u32,
        time: u32,
        extra: usize,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MouseInput {
        dx: i32,
        dy: i32,
        data: u32,
        flags: u32,
        time: u32,
        extra: usize,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    union InputUnion {
        keyboard: KeyboardInput,
        mouse: MouseInput,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Input {
        kind: u32,
        input: InputUnion,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(
            callback: Option<unsafe extern "system" fn(isize, isize) -> i32>,
            param: isize,
        ) -> i32;
        fn IsWindow(hwnd: isize) -> i32;
        fn IsWindowVisible(hwnd: isize) -> i32;
        fn IsIconic(hwnd: isize) -> i32;
        fn GetWindowTextLengthW(hwnd: isize) -> i32;
        fn GetWindowTextW(hwnd: isize, text: *mut u16, count: i32) -> i32;
        fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
        fn GetWindow(hwnd: isize, command: u32) -> isize;
        fn GetForegroundWindow() -> isize;
        fn SetForegroundWindow(hwnd: isize) -> i32;
        fn ShowWindow(hwnd: isize, command: i32) -> i32;
        fn SendInput(count: u32, inputs: *const Input, size: i32) -> u32;
        fn GetAsyncKeyState(key: i32) -> i16;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn GetCurrentProcessId() -> u32;
        fn QueryFullProcessImageNameW(
            handle: isize,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn OleInitialize(reserved: *mut c_void) -> i32;
        fn OleUninitialize();
    }
    #[link(name = "oleaut32")]
    unsafe extern "system" {
        fn SafeArrayGetLBound(array: *const c_void, dimension: u32, bound: *mut i32) -> i32;
        fn SafeArrayGetUBound(array: *const c_void, dimension: u32, bound: *mut i32) -> i32;
        fn SafeArrayGetElement(array: *const c_void, index: *const i32, value: *mut c_void) -> i32;
        fn SafeArrayDestroy(array: *const c_void) -> i32;
    }
    fn automation_error(error: impl std::fmt::Display) -> String {
        format!("无法访问目标输入框：{error}。请确认目标聊天已打开，并检查目标应用权限后重试。")
    }
    struct Apartment;
    impl Apartment {
        fn init() -> Result<Self, String> {
            let result = unsafe { OleInitialize(std::ptr::null_mut()) };
            if result < 0 {
                return Err(format!("初始化 Windows 自动化失败：0x{result:08x}"));
            }
            Ok(Self)
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                OleUninitialize();
            }
        }
    }
    fn automation() -> Result<IUIAutomation, String> {
        unsafe {
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(automation_error)
        }
    }
    fn handle(target: &Target) -> Result<isize, String> {
        let value = target.hwnd.parse::<isize>().map_err(|_| "目标窗口无效。")?;
        let mut pid = 0;
        unsafe {
            if IsWindow(value) == 0 {
                return Err("目标窗口已关闭，请重新选择。".into());
            }
            GetWindowThreadProcessId(value, &mut pid);
        }
        if pid != target.pid {
            return Err("目标窗口已变化，请重新选择。".into());
        }
        Ok(value)
    }
    fn title(hwnd: isize) -> String {
        unsafe {
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 || len > 32768 {
                return String::new();
            }
            let mut text = vec![0u16; len as usize + 1];
            let read = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
            String::from_utf16_lossy(&text[..read.max(0) as usize])
        }
    }
    fn process_name(pid: u32) -> String {
        unsafe {
            let process = OpenProcess(0x1000, 0, pid);
            if process == 0 {
                return "Windows 应用".into();
            }
            let mut buffer = vec![0u16; 32768];
            let mut len = buffer.len() as u32;
            let success = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut len);
            CloseHandle(process);
            if success == 0 {
                return "Windows 应用".into();
            }
            let full = String::from_utf16_lossy(&buffer[..len as usize]);
            Path::new(&full)
                .file_name()
                .map(|v| v.to_string_lossy().into_owned())
                .unwrap_or(full)
        }
    }
    unsafe extern "system" fn enum_window(hwnd: isize, param: isize) -> i32 {
        if unsafe { IsWindowVisible(hwnd) } == 0 || unsafe { GetWindow(hwnd, 4) } != 0 {
            return 1;
        }
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
        }
        if pid == unsafe { GetCurrentProcessId() } || pid == 0 {
            return 1;
        }
        let text = title(hwnd);
        if text.trim().is_empty() {
            return 1;
        }
        let results = unsafe { &mut *(param as *mut Vec<Target>) };
        results.push(Target {
            hwnd: hwnd.to_string(),
            pid,
            title: text,
            process: process_name(pid),
            bound: false,
        });
        1
    }
    pub fn list_targets() -> Vec<Target> {
        let mut results = Vec::new();
        unsafe {
            EnumWindows(Some(enum_window), &mut results as *mut _ as isize);
        }
        results
    }
    pub fn foreground_target() -> Option<Target> {
        let hwnd = unsafe { GetForegroundWindow() }.to_string();
        let windows = list_targets();
        windows
            .iter()
            .find(|w| w.hwnd == hwnd && w.process.to_lowercase().contains("codex"))
            .cloned()
            .or_else(|| {
                windows
                    .into_iter()
                    .find(|w| w.process.to_lowercase().contains("codex"))
            })
    }
    pub fn validate_target(target: &Target) -> Result<(), String> {
        handle(target).map(|_| ())
    }
    pub fn foreground(target: &Target) -> Result<(), String> {
        let hwnd = handle(target)?;
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, 9);
            }
            if GetForegroundWindow() != hwnd && SetForegroundWindow(hwnd) == 0 {
                return Err("Windows 未允许切换到目标窗口，请先点击一次目标窗口再重试。".into());
            }
        }
        Ok(())
    }
    fn runtime_id(element: &IUIAutomationElement) -> Result<Vec<i32>, String> {
        unsafe {
            let array = element.GetRuntimeId().map_err(automation_error)?;
            if array.is_null() {
                return Err("输入框未提供可绑定的标识。".into());
            }
            let mut lower = 0;
            let mut upper = -1;
            let low = SafeArrayGetLBound(array.cast(), 1, &mut lower);
            let high = SafeArrayGetUBound(array.cast(), 1, &mut upper);
            if low < 0 || high < 0 || upper < lower || upper - lower > 256 {
                SafeArrayDestroy(array.cast());
                return Err("输入框标识不可用，将在操作时自动重新查找。".into());
            }
            let mut values = Vec::new();
            for index in lower..=upper {
                let mut item = 0i32;
                if SafeArrayGetElement(array.cast(), &index, (&mut item as *mut i32).cast()) < 0 {
                    SafeArrayDestroy(array.cast());
                    return Err("无法读取输入框标识。".into());
                }
                values.push(item);
            }
            SafeArrayDestroy(array.cast());
            Ok(values)
        }
    }
    fn is_editable(element: &IUIAutomationElement) -> bool {
        unsafe {
            let kind = element.CurrentControlType().ok();
            let is_input =
                kind == Some(UIA_EditControlTypeId) || kind == Some(UIA_DocumentControlTypeId);
            is_input
                && element.CurrentIsEnabled().is_ok_and(|v| v.as_bool())
                && element
                    .CurrentIsKeyboardFocusable()
                    .is_ok_and(|v| v.as_bool())
                && element.CurrentIsOffscreen().is_ok_and(|v| !v.as_bool())
                && element.CurrentIsPassword().is_ok_and(|v| !v.as_bool())
        }
    }
    fn find_input(
        client: &IUIAutomation,
        target: &Target,
        binding: Option<&Binding>,
    ) -> Result<(IUIAutomationElement, Option<Binding>), String> {
        unsafe {
            let hwnd = handle(target)?;
            let root = client
                .ElementFromHandle(HWND(hwnd as *mut c_void))
                .map_err(automation_error)?;
            let edits = client
                .CreatePropertyCondition(
                    UIA_ControlTypePropertyId,
                    &VARIANT::from(UIA_EditControlTypeId.0),
                )
                .map_err(automation_error)?;
            let documents = client
                .CreatePropertyCondition(
                    UIA_ControlTypePropertyId,
                    &VARIANT::from(UIA_DocumentControlTypeId.0),
                )
                .map_err(automation_error)?;
            let condition = client
                .CreateOrCondition(&edits, &documents)
                .map_err(automation_error)?;
            let elements = root
                .FindAll(TreeScope_Descendants, &condition)
                .map_err(automation_error)?;
            let count = elements.Length().map_err(automation_error)?.min(2000);
            let mut candidates = Vec::new();
            let mut inputs = Vec::new();
            for index in 0..count {
                let Ok(element) = elements.GetElement(index) else {
                    continue;
                };
                if !is_editable(&element) {
                    continue;
                }
                let value = element
                    .GetCurrentPattern(UIA_ValuePatternId)
                    .and_then(|pattern| pattern.cast::<IUIAutomationValuePattern>())
                    .ok();
                candidates.push(Candidate {
                    runtime_id: runtime_id(&element).ok(),
                    kind: if element.CurrentControlType().ok() == Some(UIA_EditControlTypeId) {
                        InputKind::Edit
                    } else {
                        InputKind::Document
                    },
                    name: element.CurrentName().unwrap_or_default().to_string(),
                    automation_id: element
                        .CurrentAutomationId()
                        .unwrap_or_default()
                        .to_string(),
                    help_text: element.CurrentHelpText().unwrap_or_default().to_string(),
                    class_name: element.CurrentClassName().unwrap_or_default().to_string(),
                    read_only: value.as_ref().is_some_and(|pattern| {
                        pattern
                            .CurrentIsReadOnly()
                            .is_ok_and(|value| value.as_bool())
                    }),
                    has_value_pattern: value.is_some(),
                    has_text_edit_pattern: element.GetCurrentPattern(UIA_TextEditPatternId).is_ok(),
                    focused: element
                        .CurrentHasKeyboardFocus()
                        .is_ok_and(|value| value.as_bool()),
                });
                inputs.push(element);
            }
            let index = input::choose(
                &candidates,
                binding.map(|bound| bound.runtime_id.as_slice()),
            )?;
            let binding = candidates[index]
                .runtime_id
                .clone()
                .map(|runtime_id| Binding { runtime_id });
            Ok((inputs.swap_remove(index), binding))
        }
    }
    fn modifiers_released() -> Result<(), String> {
        let started = Instant::now();
        while [0x10, 0x11, 0x12, 0x5B, 0x5C]
            .iter()
            .any(|&key| unsafe { GetAsyncKeyState(key) } < 0)
        {
            if started.elapsed() > Duration::from_millis(1800) {
                return Err("请松开 Ctrl、Alt、Shift 和 Win 键后再试。".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }
    fn key(vk: u16, up: bool) -> Input {
        Input {
            kind: 1,
            input: InputUnion {
                keyboard: KeyboardInput {
                    vk,
                    scan: 0,
                    flags: if up { 2 } else { 0 },
                    time: 0,
                    extra: 0,
                },
            },
        }
    }
    fn keys(inputs: &[Input]) -> Result<(), String> {
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                size_of::<Input>() as i32,
            )
        };
        if sent != inputs.len() as u32 {
            return Err("Windows 未能完整发送按键。请检查目标应用是否以管理员身份运行。".into());
        }
        Ok(())
    }
    fn verify_focus(
        client: &IUIAutomation,
        target: &Target,
        input: &IUIAutomationElement,
    ) -> Result<(), String> {
        if unsafe { GetForegroundWindow() } != handle(target)? {
            return Err("目标窗口已失去焦点，已停止输入。".into());
        }
        let focused = unsafe { client.GetFocusedElement().map_err(automation_error)? };
        if !unsafe {
            client
                .CompareElements(&focused, input)
                .map_err(automation_error)?
        }
        .as_bool()
        {
            return Err("输入焦点发生变化，已停止输入。".into());
        }
        Ok(())
    }
    fn focus_input(
        client: &IUIAutomation,
        target: &Target,
        input: &IUIAutomationElement,
    ) -> Result<(), String> {
        unsafe {
            input.SetFocus().map_err(automation_error)?;
        }
        thread::sleep(Duration::from_millis(70));
        verify_focus(client, target, input)
    }
    pub fn execute(
        target: &Target,
        binding: Option<&Binding>,
        text: &str,
        action: Action,
        send_key: SendKey,
    ) -> Result<ExecutionResult, String> {
        let _apartment = Apartment::init()?;
        modifiers_released()?;
        foreground(target)?;
        thread::sleep(Duration::from_millis(100));
        let client = automation()?;
        let (mut input, mut binding) = find_input(&client, target, binding)?;
        if focus_input(&client, target, &input).is_err() {
            // The provider can rebuild the editor between discovery and focusing.
            // Rediscover once before issuing any keyboard input.
            (input, binding) = find_input(&client, target, None)?;
            focus_input(&client, target, &input)?;
        }
        let mut clipboard = crate::clipboard::ClipboardSnapshot::paste_text(text)?;
        let result = (|| {
            modifiers_released()?;
            verify_focus(&client, target, &input)?;
            keys(&[
                key(0x11, false),
                key(0x56, false),
                key(0x56, true),
                key(0x11, true),
            ])?;
            // Rich editors can expose the whole document or stale text through UIA.
            // Allow paste handling to run without treating that text as a draft or a gate.
            thread::sleep(Duration::from_millis(300));
            if action == Action::Insert {
                return Ok(ExecutionResult {
                    message: "已填入目标输入框，尚未发送".into(),
                    binding,
                });
            }
            modifiers_released()?;
            verify_focus(&client, target, &input)?;
            match send_key {
                SendKey::Enter => keys(&[key(0x0D, false), key(0x0D, true)])?,
                SendKey::CtrlEnter => keys(&[
                    key(0x11, false),
                    key(0x0D, false),
                    key(0x0D, true),
                    key(0x11, true),
                ])?,
            }
            thread::sleep(Duration::from_millis(100));
            Ok(ExecutionResult {
                message: "已填入并触发发送".into(),
                binding,
            })
        })();
        clipboard.finish(result)
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn keyboard_input_abi_matches_windows() {
            assert_eq!(
                size_of::<Input>(),
                if cfg!(target_pointer_width = "64") {
                    40
                } else {
                    28
                }
            );
        }
    }
}
#[cfg(windows)]
pub use win::{execute, foreground_target, list_targets, validate_target};

#[cfg(not(windows))]
pub fn list_targets() -> Vec<Target> {
    vec![]
}
#[cfg(not(windows))]
pub fn foreground_target() -> Option<Target> {
    None
}
#[cfg(not(windows))]
pub fn validate_target(_: &Target) -> Result<(), String> {
    Err("自动发送目前只支持 Windows。".into())
}
#[cfg(not(windows))]
pub fn execute(
    _: &Target,
    _: Option<&Binding>,
    _: &str,
    _: Action,
    _: SendKey,
) -> Result<ExecutionResult, String> {
    Err("自动发送目前只支持 Windows。".into())
}
