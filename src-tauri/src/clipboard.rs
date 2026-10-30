//! An owned, eagerly materialized clipboard backup for temporary paste operations.
//! No clipboard IDataObject reference survives replacement of its original contents.
use std::{ffi::c_void, ptr, thread, time::Duration};

const CF_UNICODETEXT: u32 = 13;

#[link(name = "user32")]
unsafe extern "system" {
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: isize,
        menu: isize,
        instance: isize,
        param: *const c_void,
    ) -> isize;
    fn DestroyWindow(window: isize) -> i32;
    fn OpenClipboard(window: isize) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn EnumClipboardFormats(format: u32) -> u32;
    fn GetClipboardData(format: u32) -> isize;
    fn SetClipboardData(format: u32, data: isize) -> isize;
    fn GetClipboardOwner() -> isize;
    fn GetClipboardFormatNameW(format: u32, name: *mut u16, count: i32) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalLock(handle: isize) -> *mut c_void;
    fn GlobalUnlock(handle: isize) -> i32;
    fn GetLastError() -> u32;
    fn SetLastError(error: u32);
}
#[link(name = "ole32")]
unsafe extern "system" {
    fn OleDuplicateData(source: isize, format: u16, flags: u32) -> isize;
    fn ReleaseStgMedium(medium: *mut Medium);
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CopyEnhMetaFileW(source: isize, file: *const u16) -> isize;
}
// STGMEDIUM contains a DWORD, a pointer-sized union, and an IUnknown pointer.
#[repr(C)]
struct Medium {
    kind: u32,
    handle: isize,
    release: *mut c_void,
}

struct Window(isize);
impl Window {
    fn new() -> Result<Self, String> {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let handle = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                0,
                0,
                0,
                0,
                0,
                -3,
                0,
                0,
                ptr::null(),
            )
        };
        if handle == 0 {
            return Err("无法创建剪贴板备份窗口。".into());
        }
        Ok(Self(handle))
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0);
        }
    }
}
struct Open;
impl Open {
    fn new(window: isize) -> Result<Self, String> {
        for _ in 0..40 {
            if unsafe { OpenClipboard(window) } != 0 {
                return Ok(Self);
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err("剪贴板持续被其他程序占用，请稍后重试。".into())
    }
}
impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

struct Data {
    format: u32,
    handle: isize,
}
impl Data {
    fn memory(format: u32, bytes: &[u8]) -> Result<Self, String> {
        let handle = unsafe { GlobalAlloc(0x42, bytes.len().max(1)) };
        if handle == 0 {
            return Err("无法分配剪贴板备份内存。".into());
        }
        let data = Self { format, handle };
        let pointer = unsafe { GlobalLock(handle) };
        if pointer.is_null() {
            return Err("无法读取剪贴板备份内存。".into());
        }
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast(), bytes.len());
            GlobalUnlock(handle);
        }
        Ok(data)
    }
    fn text(text: &str) -> Result<Self, String> {
        let bytes: Vec<u8> = text
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        Self::memory(CF_UNICODETEXT, &bytes)
    }
    fn duplicate(format: u32, handle: isize) -> Result<Self, String> {
        let format_kind = match format {
            0x82 => 2,
            0x83 => 3,
            0x8e => 14,
            other => other,
        };
        let copy = unsafe {
            if format_kind == 14 {
                CopyEnhMetaFileW(handle, ptr::null())
            } else {
                OleDuplicateData(handle, format_kind as u16, 2)
            }
        };
        if copy == 0 {
            return Err(format!("无法复制剪贴板格式 {format}。"));
        }
        Ok(Self {
            format,
            handle: copy,
        })
    }
    fn copy(&self) -> Result<Self, String> {
        Self::duplicate(self.format, self.handle)
    }
    // Caller owns the open clipboard. On success Windows owns the handle.
    fn publish(&mut self) -> Result<(), String> {
        if unsafe { SetClipboardData(self.format, self.handle) } == 0 {
            return Err(format!("无法写入剪贴板格式 {}。", self.format));
        }
        self.handle = 0;
        Ok(())
    }
}
impl Drop for Data {
    fn drop(&mut self) {
        if self.handle == 0 {
            return;
        }
        let kind = match self.format {
            2 | 9 | 0x82 => 16,
            3 | 0x83 => 32,
            14 | 0x8e => 64,
            _ => 1,
        };
        unsafe {
            ReleaseStgMedium(&mut Medium {
                kind,
                handle: self.handle,
                release: ptr::null_mut(),
            });
        }
    }
}
fn internal_format(format: u32) -> bool {
    let mut name = [0u16; 128];
    let length = unsafe { GetClipboardFormatNameW(format, name.as_mut_ptr(), name.len() as i32) };
    // These OLE bookkeeping formats contain references to the old clipboard object,
    // rather than transferable user data. Re-publishing them would revive stale pointers.
    matches!(
        String::from_utf16_lossy(&name[..length.max(0) as usize]).as_str(),
        "DataObject" | "Ole Private Data"
    )
}
fn capture() -> Result<Vec<Data>, String> {
    let mut formats = Vec::new();
    let mut format = 0;
    loop {
        unsafe {
            SetLastError(0);
        }
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            if unsafe { GetLastError() } != 0 {
                return Err("无法枚举剪贴板内容，未替换剪贴板。".into());
            }
            break;
        }
        formats.push(format);
    }
    let mut previous = Vec::new();
    for format in formats {
        if internal_format(format) {
            continue;
        }
        if format == 0x80 || (0x200..=0x2ff).contains(&format) {
            return Err("当前剪贴板含无法安全备份的专用格式，未替换剪贴板。".into());
        }
        // Request delayed-rendered formats now, while their original owner still exists.
        let handle = unsafe { GetClipboardData(format) };
        if handle == 0 {
            return Err(format!("无法读取剪贴板格式 {format}，未替换剪贴板。"));
        }
        previous.push(Data::duplicate(format, handle)?);
    }
    Ok(previous)
}

pub struct ClipboardSnapshot {
    previous: Vec<Data>,
    window: Window,
    armed: bool,
}
impl ClipboardSnapshot {
    pub fn paste_text(text: &str) -> Result<Self, String> {
        let window = Window::new()?;
        let opened = Open::new(window.0)?;
        let previous = capture()?;
        let mut temporary = Data::text(text)?;
        let mut snapshot = Self {
            previous,
            window,
            armed: false,
        };
        let result = if unsafe { EmptyClipboard() } == 0 {
            Err("无法临时写入剪贴板，原内容未替换。".into())
        } else {
            snapshot.armed = true;
            temporary.publish()
        };
        // Always close before error unwinding can invoke the restoration guard.
        drop(opened);
        result?;
        Ok(snapshot)
    }
    fn restore_once(&mut self) -> Result<(), String> {
        let _opened = Open::new(self.window.0)?;
        // Ownership is checked under the clipboard lock. A consumer may synthesize
        // ANSI/DIB formats (changing the sequence) without replacing our clipboard.
        // A newer copy from another window must always be left alone.
        if unsafe { GetClipboardOwner() } != self.window.0 {
            self.armed = false;
            return Ok(());
        }
        // Keep the originals until every format has been restored, so a partial
        // SetClipboardData failure can be retried without losing the backup.
        let mut copies = self
            .previous
            .iter()
            .map(Data::copy)
            .collect::<Result<Vec<_>, _>>()?;
        if unsafe { EmptyClipboard() } == 0 {
            return Err("无法恢复原剪贴板内容。".into());
        }
        for data in &mut copies {
            data.publish()?;
        }
        self.armed = false;
        Ok(())
    }
    pub fn restore(&mut self) -> Result<(), String> {
        if !self.armed {
            return Ok(());
        }
        let mut error = String::new();
        for _ in 0..3 {
            match self.restore_once() {
                Ok(()) => return Ok(()),
                Err(message) => error = message,
            }
            thread::sleep(Duration::from_millis(30));
        }
        Err(format!("恢复原剪贴板失败：{error}"))
    }
    pub fn finish<T>(&mut self, result: Result<T, String>) -> Result<T, String> {
        match (result, self.restore()) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(format!("输入操作已执行，但{error}")),
            (Err(error), Err(restore_error)) => Err(format!("{error}；{restore_error}")),
        }
    }
}
impl Drop for ClipboardSnapshot {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalSize(handle: isize) -> usize;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateBitmap(
            width: i32,
            height: i32,
            planes: u32,
            bits: u32,
            data: *const c_void,
        ) -> isize;
        fn GetBitmapBits(bitmap: isize, count: i32, data: *mut c_void) -> i32;
    }
    fn read(data: &Data, size: usize) -> Vec<u8> {
        unsafe {
            assert!(GlobalSize(data.handle) >= size);
            let pointer = GlobalLock(data.handle);
            assert!(!pointer.is_null());
            let bytes = std::slice::from_raw_parts(pointer.cast::<u8>(), size).to_vec();
            GlobalUnlock(data.handle);
            bytes
        }
    }
    fn registered(name: &str) -> u32 {
        let name: Vec<_> = name.encode_utf16().chain([0]).collect();
        let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
        assert_ne!(format, 0);
        format
    }
    fn verify_copy(format: u32, bytes: &[u8]) {
        let source = Data::memory(format, bytes).unwrap();
        let copy = source.copy().unwrap();
        assert_ne!(copy.handle, source.handle);
        drop(source);
        assert_eq!(read(&copy, bytes.len()), bytes);
    }
    #[test]
    fn unicode_backup_owns_bytes_after_the_original_is_freed() {
        let text = "原剪贴板🙂\r\nsecond line";
        let source = Data::text(text).unwrap();
        let copy = source.copy().unwrap();
        drop(source);
        let expected: Vec<_> = text
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(read(&copy, expected.len()), expected);
    }
    #[test]
    fn registered_rich_text_and_binary_formats_are_copied_exactly() {
        verify_copy(
            registered("HTML Format"),
            b"Version:1.0\r\n<html><b>original</b></html>\0",
        );
        verify_copy(registered("Rich Text Format"), b"{\\rtf1 original}\0");
        verify_copy(
            registered("PromptDock.ClipboardRegression"),
            &[0, 255, 1, 128, 0],
        );
    }
    #[test]
    fn bitmap_and_file_list_memory_survive_the_original_handle() {
        verify_copy(8, &dib());
        verify_copy(15, &files());
    }
    #[test]
    fn gdi_bitmap_is_duplicated_as_a_gdi_object() {
        let pixel = [0x11u8, 0x22, 0x33, 0];
        let source = Data {
            format: 2,
            handle: unsafe { CreateBitmap(1, 1, 1, 32, pixel.as_ptr().cast()) },
        };
        assert_ne!(source.handle, 0);
        let copy = source.copy().unwrap();
        let mut original_bits = [0u8; 4];
        unsafe {
            assert_eq!(
                GetBitmapBits(source.handle, 4, original_bits.as_mut_ptr().cast()),
                4
            );
        }
        drop(source);
        let mut copy_bits = [0u8; 4];
        unsafe {
            assert_eq!(
                GetBitmapBits(copy.handle, 4, copy_bits.as_mut_ptr().cast()),
                4
            );
        }
        assert_eq!(copy_bits, original_bits);
    }
    fn dib() -> Vec<u8> {
        let mut data = vec![0u8; 44];
        data[0..4].copy_from_slice(&40u32.to_le_bytes());
        data[4..8].copy_from_slice(&1i32.to_le_bytes());
        data[8..12].copy_from_slice(&1i32.to_le_bytes());
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&32u16.to_le_bytes());
        data[20..24].copy_from_slice(&4u32.to_le_bytes());
        data[40..44].copy_from_slice(&[0x11, 0x22, 0x33, 0xff]);
        data
    }
    fn files() -> Vec<u8> {
        let mut data = vec![0u8; 20];
        data[0..4].copy_from_slice(&20u32.to_le_bytes());
        data[16..20].copy_from_slice(&1u32.to_le_bytes());
        data.extend(
            "C:\\PromptDock-test.txt\0\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        data
    }
    // Opt-in integration test: uses the real Windows clipboard for a short time.
    // Retain an owned backup and every fixture window. Even on panic, restore the
    // user's original only while a fixture still owns the clipboard. Never print it.
    struct Fixtures {
        saved: ClipboardSnapshot,
        operations: Vec<ClipboardSnapshot>,
        windows: Vec<Window>,
    }
    impl Fixtures {
        fn new() -> Self {
            Self {
                saved: ClipboardSnapshot::paste_text("PromptDock clipboard regression").unwrap(),
                operations: vec![],
                windows: vec![],
            }
        }
        fn owns(&self, owner: isize) -> bool {
            owner == self.saved.window.0
                || self.operations.iter().any(|s| s.window.0 == owner)
                || self.windows.iter().any(|w| w.0 == owner)
        }
        fn seed(&mut self, mut data: Vec<Data>) {
            let _opened = Open::new(self.saved.window.0).unwrap();
            assert!(
                self.owns(unsafe { GetClipboardOwner() }),
                "clipboard changed externally; stopping test"
            );
            unsafe {
                assert_ne!(EmptyClipboard(), 0);
            }
            for item in &mut data {
                item.publish().unwrap();
            }
        }
        fn start(&mut self) -> usize {
            let snapshot = ClipboardSnapshot::paste_text("temporary prompt 测试🙂").unwrap();
            self.operations.push(snapshot);
            self.operations.len() - 1
        }
        fn check(&self, format: u32, expected: &[u8]) {
            let _opened = Open::new(self.saved.window.0).unwrap();
            let handle = unsafe { GetClipboardData(format) };
            assert_ne!(handle, 0, "missing clipboard format {format}");
            let data = Data::duplicate(format, handle).unwrap();
            assert_eq!(read(&data, expected.len()), expected);
        }
        fn check_text(&self, expected: &str) {
            let bytes: Vec<_> = expected
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            self.check(CF_UNICODETEXT, &bytes);
        }
    }
    impl Drop for Fixtures {
        fn drop(&mut self) {
            let owner = unsafe { GetClipboardOwner() };
            if owner != self.saved.window.0 {
                if let Some(snapshot) = self.operations.iter_mut().find(|s| s.window.0 == owner) {
                    std::mem::swap(&mut self.saved.window, &mut snapshot.window);
                } else if let Some(window) = self.windows.iter_mut().find(|w| w.0 == owner) {
                    std::mem::swap(&mut self.saved.window, window);
                }
            }
            if let Err(error) = self.saved.restore() {
                eprintln!("test cleanup: {error}");
            }
        }
    }
    #[test]
    #[ignore = "temporarily uses the real clipboard; preserves original data with an owned guard"]
    fn native_clipboard_transaction_restores_content_and_respects_newer_copies() {
        let mut fixture = Fixtures::new();
        // Plain text, synthesized ANSI formats, and repeated insertion.
        for _ in 0..3 {
            fixture.seed(vec![Data::text("original 中文🙂\r\nline 2").unwrap()]);
            let index = fixture.start();
            {
                let _opened = Open::new(fixture.saved.window.0).unwrap();
                unsafe {
                    assert_ne!(GetClipboardData(1), 0);
                }
            }
            fixture.operations[index].finish(Ok(())).unwrap();
            fixture.check_text("original 中文🙂\r\nline 2");
        }
        // Empty clipboard remains empty.
        fixture.seed(vec![]);
        let index = fixture.start();
        fixture.operations[index].restore().unwrap();
        {
            let _opened = Open::new(fixture.saved.window.0).unwrap();
            assert!(capture().unwrap().is_empty());
        }
        // A single clipboard item can contain multiple independent formats.
        let html = registered("HTML Format");
        let rtf = registered("Rich Text Format");
        let custom = registered("PromptDock.ClipboardRegression");
        fixture.seed(vec![
            Data::text("rich original").unwrap(),
            Data::memory(html, b"<b>original</b>\0").unwrap(),
            Data::memory(rtf, b"{\\rtf1 original}\0").unwrap(),
            Data::memory(custom, &[0, 255, 1, 128]).unwrap(),
        ]);
        let index = fixture.start();
        fixture.operations[index].restore().unwrap();
        fixture.check_text("rich original");
        fixture.check(html, b"<b>original</b>\0");
        fixture.check(rtf, b"{\\rtf1 original}\0");
        fixture.check(custom, &[0, 255, 1, 128]);
        // Images and file selections are preserved without converting them to text.
        for (format, bytes) in [(8, dib()), (15, files())] {
            fixture.seed(vec![Data::memory(format, &bytes).unwrap()]);
            let index = fixture.start();
            fixture.operations[index].restore().unwrap();
            fixture.check(format, &bytes);
        }
        // Error paths still restore, and propagate the original operation error.
        fixture.seed(vec![Data::text("error fixture").unwrap()]);
        let index = fixture.start();
        assert_eq!(
            fixture.operations[index]
                .finish::<()>(Err("fixture focus error".into()))
                .unwrap_err(),
            "fixture focus error"
        );
        fixture.check_text("error fixture");
        // Short clipboard contention is retried.
        fixture.seed(vec![Data::text("busy fixture").unwrap()]);
        let index = fixture.start();
        let (tx, rx) = std::sync::mpsc::channel();
        let holder = thread::spawn(move || {
            let window = Window::new().unwrap();
            let _opened = Open::new(window.0).unwrap();
            tx.send(()).unwrap();
            thread::sleep(Duration::from_millis(100));
        });
        rx.recv().unwrap();
        fixture.operations[index].restore().unwrap();
        holder.join().unwrap();
        fixture.check_text("busy fixture");
        // A newer copy by another owner must win over the saved previous contents.
        fixture.seed(vec![Data::text("before external copy").unwrap()]);
        let index = fixture.start();
        let window = Window::new().unwrap();
        {
            let _opened = Open::new(window.0).unwrap();
            unsafe {
                assert_ne!(EmptyClipboard(), 0);
            }
            Data::text("new external copy").unwrap().publish().unwrap();
        }
        fixture.windows.push(window);
        fixture.operations[index].restore().unwrap();
        fixture.check_text("new external copy");
    }
}
