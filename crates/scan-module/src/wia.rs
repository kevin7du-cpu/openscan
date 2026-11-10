//! All Windows FFI is confined to this module. COM pointers never cross threads.
use super::*;
use std::sync::Mutex;
use windows::{
    Win32::{
        Devices::ImageAcquisition::*,
        Foundation::{E_ABORT, E_FAIL, E_POINTER, HWND, S_FALSE},
        System::Com::{
            CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize, IStream, STGM_READWRITE, STGM_SHARE_EXCLUSIVE,
            StructuredStorage::{
                PROPSPEC, PROPSPEC_0, PROPVARIANT, PRSPEC_PROPID, PropVariantClear,
                PropVariantToInt32, PropVariantToInt32Vector, PropVariantToStringAlloc,
            },
        },
        UI::Shell::SHCreateStreamOnFileEx,
    },
    core::{BSTR, HSTRING, Interface, implement},
};
static HARDWARE: Mutex<()> = Mutex::new(());
pub struct WiaBackend;
fn error(e: windows::core::Error) -> ScanError {
    let message = match e.code().0 as u32 {
        0x80210002 => "Paper jam; clear the scanner. Completed pages are retained.",
        0x80210003 => "The feeder is empty. Load documents and try again.",
        0x80210005 => "Scanner offline; check power and connection.",
        0x80210006 => "Scanner is busy in another application.",
        0x80210016 => "Scanner cover is open.",
        _ => return ScanError::Backend(e.to_string()),
    };
    ScanError::Backend(message.into())
}
struct Apartment;
impl Apartment {
    fn start() -> Result<Self> {
        // SAFETY: Initialize COM on this calling thread; Apartment balances successful initialization.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .map_err(error)?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: apartment is created and dropped on this same stack/thread.
        unsafe {
            CoUninitialize();
        }
    }
}
fn manager() -> Result<IWiaDevMgr2> {
    // SAFETY: caller owns an initialized apartment; returned interface is reference counted.
    unsafe { CoCreateInstance(&WiaDevMgr2, None, CLSCTX_LOCAL_SERVER).map_err(error) }
}
struct Property(PROPVARIANT);
impl Drop for Property {
    fn drop(&mut self) {
        // SAFETY: initialized to empty or populated by ReadMultiple, cleared once.
        unsafe {
            let _ = PropVariantClear(&mut self.0);
        }
    }
}
fn property(storage: &IWiaPropertyStorage, id: u32) -> Result<String> {
    let spec = PROPSPEC {
        ulKind: PRSPEC_PROPID,
        Anonymous: PROPSPEC_0 { propid: id },
    };
    let mut value = Property(PROPVARIANT::default());
    // SAFETY: pointers reference live, correctly sized stack values; output is freed with its documented allocator.
    unsafe {
        storage
            .ReadMultiple(1, &spec, &mut value.0)
            .map_err(error)?;
        let text = PropVariantToStringAlloc(&value.0).map_err(error)?;
        let result = text
            .to_string()
            .map_err(|e| ScanError::Backend(e.to_string()));
        CoTaskMemFree(Some(text.0.cast()));
        result
    }
}
struct ReturnedPaths {
    pointer: *mut BSTR,
    count: i32,
}
impl Drop for ReturnedPaths {
    fn drop(&mut self) {
        if self.pointer.is_null() {
            return;
        }
        // SAFETY: WIA allocates count BSTR values with SysAllocString and the array with CoTaskMemAlloc.
        // BSTR::drop frees each string; CoTaskMemFree frees only the containing array.
        unsafe {
            for index in 0..self.count.max(0) as usize {
                std::ptr::drop_in_place(self.pointer.add(index));
            }
            CoTaskMemFree(Some(self.pointer.cast()));
        }
    }
}
impl ScannerBackend for WiaBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            name: "Windows WIA 2",
            driver_dialog: true,
            programmatic_settings: true,
            cancellation_note: "Direct scans cancel at driver callbacks; cancel vendor dialogs inside that dialog. Completed pages are retained.",
        }
    }
    fn devices(&self) -> Result<Vec<Device>> {
        let _apartment = Apartment::start()?;
        let manager = manager()?;
        // SAFETY: all interfaces and out-parameters remain inside the initialized apartment.
        unsafe {
            let enumerator = manager
                .EnumDeviceInfo(WIA_DEVINFO_ENUM_LOCAL as i32)
                .map_err(error)?;
            let mut devices = Vec::new();
            loop {
                let mut storage = None;
                let mut count = 0;
                enumerator
                    .Next(1, &mut storage, &mut count)
                    .map_err(error)?;
                if count == 0 {
                    break;
                }
                if let Some(storage) = storage {
                    devices.push(Device {
                        id: property(&storage, WIA_DIP_DEV_ID)?,
                        name: property(&storage, WIA_DIP_DEV_NAME)?,
                    });
                }
            }
            Ok(devices)
        }
    }
    fn acquire(
        &self,
        request: &ScanRequest,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<ScanBatch> {
        cancel.check()?;
        validate_request(request)?;
        let _hardware = HARDWARE
            .try_lock()
            .map_err(|_| ScanError::Backend("Another scan is in progress".into()))?;
        if !request.settings.native_interface {
            return acquire_direct(request, cancel, progress);
        }
        let output = request.output_dir.canonicalize()?;
        let _apartment = Apartment::start()?;
        let manager = manager()?;
        progress(Progress::Connecting);
        let mut returned = ReturnedPaths {
            pointer: std::ptr::null_mut(),
            count: 0,
        };
        let mut item: Option<IWiaItem2> = None;
        let device = request
            .device_id
            .as_deref()
            .map(BSTR::from)
            .unwrap_or_default();
        let folder = BSTR::from(output.to_string_lossy().as_ref());
        let filename = BSTR::from("scan.bmp");
        progress(Progress::Acquiring);
        // SAFETY: all BSTR arguments and writable out-parameters live through the blocking call.
        // ReturnedPaths frees results even if WIA reports an error or partial cancellation.
        let result = unsafe {
            (manager.vtable().GetImageDlg)(
                manager.as_raw(),
                0,
                device.as_ptr().cast_mut().cast(),
                HWND::default(),
                folder.as_ptr().cast_mut().cast(),
                filename.as_ptr().cast_mut().cast(),
                &mut returned.count,
                (&mut returned.pointer as *mut *mut BSTR).cast(),
                (&mut item as *mut Option<IWiaItem2>).cast(),
            )
        };
        let mut pages = Vec::new();
        if !returned.pointer.is_null() {
            // SAFETY: successful WIA call supplies count initialized BSTR entries.
            for index in 0..returned.count.max(0) as usize {
                let path = unsafe { PathBuf::from((*returned.pointer.add(index)).to_string()) };
                let path = path.canonicalize()?;
                if !path.starts_with(&output) {
                    return Err(ScanError::Backend(
                        "Scanner returned a file outside the requested output directory".into(),
                    ));
                }
                progress(Progress::PageReady(path.clone()));
                if pages.len() < request.settings.max_pages as usize {
                    pages.push(path);
                }
            }
        }
        if pages.is_empty() {
            result.ok().map_err(error)?;
            return Err(ScanError::Cancelled);
        }
        Ok(ScanBatch {
            pages,
            cancelled: result == S_FALSE || cancel.is_cancelled(),
            warning: result.ok().err().map(|e| error(e).to_string()),
            dpi: None,
        })
    }
    fn device_capabilities(&self, device_id: &str) -> Result<DeviceCapabilities> {
        let _apartment = Apartment::start()?;
        // SAFETY: every interface lives in this initialized calling apartment.
        let root = unsafe {
            manager()?
                .CreateDevice(0, &BSTR::from(device_id))
                .map_err(error)?
        };
        let storage: IWiaPropertyStorage = root.cast().map_err(error)?;
        let handling =
            integer(&storage, WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES).unwrap_or(0) as u32;
        let mut sources = Vec::new();
        for item in children(&root)? {
            // SAFETY: live WIA item in this apartment; typed GUID avoids the C# VT_CLSID limitation.
            let category = unsafe { item.GetItemCategory().map_err(error)? };
            let source = if category == WIA_CATEGORY_FLATBED {
                ScanSource::Flatbed
            } else if category == WIA_CATEGORY_FEEDER {
                ScanSource::Feeder
            } else {
                continue;
            };
            let storage: IWiaPropertyStorage = item.cast().map_err(error)?;
            let resolutions = resolutions(&storage)?;
            sources.push(SourceCapabilities {
                source,
                resolutions: resolutions.clone(),
            });
            if source == ScanSource::Feeder && handling & DUPLEX != 0 {
                sources.push(SourceCapabilities {
                    source: ScanSource::Duplex,
                    resolutions,
                });
            }
        }
        Ok(DeviceCapabilities{device_id:device_id.into(),sources,note:"Only driver-reported sources and resolutions are listed. Use the scanner interface for vendor-specific options.".into()})
    }
}

fn spec(id: u32) -> PROPSPEC {
    PROPSPEC {
        ulKind: PRSPEC_PROPID,
        Anonymous: PROPSPEC_0 { propid: id },
    }
}
fn integer(storage: &IWiaPropertyStorage, id: u32) -> Result<i32> {
    let mut value = Property(PROPVARIANT::default());
    // SAFETY: correctly sized initialized output, cleared by Property on all paths.
    unsafe {
        storage
            .ReadMultiple(1, &spec(id), &mut value.0)
            .map_err(error)?;
        PropVariantToInt32(&value.0).map_err(error)
    }
}
fn set_integer(storage: &IWiaPropertyStorage, id: u32, value: i32) -> Result<()> {
    let value = PROPVARIANT::from(value);
    // SAFETY: property and value remain alive throughout the synchronous write.
    unsafe {
        storage
            .WriteMultiple(1, &spec(id), &value, 2)
            .map_err(error)
    }
}
fn children(root: &IWiaItem2) -> Result<Vec<IWiaItem2>> {
    // SAFETY: caller has an initialized apartment; enumeration owns all returned references.
    unsafe {
        let items = root.EnumChildItems(None).map_err(error)?;
        let mut result = Vec::new();
        loop {
            let mut item = None;
            let mut count = 0;
            items.Next(1, &mut item, &mut count).map_err(error)?;
            if count == 0 {
                break;
            }
            if let Some(item) = item {
                result.push(item);
            }
        }
        Ok(result)
    }
}
fn resolutions(storage: &IWiaPropertyStorage) -> Result<Vec<u32>> {
    let mut value = Property(PROPVARIANT::default());
    let mut flags = 0;
    let mut values = [0_i32; 256];
    let mut count = 0;
    // SAFETY: bounded outputs, live property variant, no borrowed values escape.
    unsafe {
        if storage
            .GetPropertyAttributes(1, &spec(WIA_IPS_XRES), &mut flags, &mut value.0)
            .is_err()
            || PropVariantToInt32Vector(&value.0, &mut values, &mut count).is_err()
        {
            return Ok(integer(storage, WIA_IPS_XRES)
                .ok()
                .filter(|x| (75..=600).contains(x))
                .map(|x| vec![x as u32])
                .unwrap_or_default());
        }
    }
    let values = &values[..(count as usize).min(values.len())];
    let mut result = if flags & WIA_PROP_LIST != 0 && values.len() >= 2 {
        values[2..]
            .iter()
            .copied()
            .take(values[0].max(0) as usize)
            .filter(|x| (75..=600).contains(x))
            .map(|x| x as u32)
            .collect::<Vec<_>>()
    } else if flags & WIA_PROP_RANGE != 0 && values.len() >= 4 {
        [75, 100, 150, 200, 300, 400, 600]
            .into_iter()
            .filter(|x| {
                *x >= values[0] && *x <= values[2] && (*x - values[0]) % values[3].max(1) == 0
            })
            .map(|x| x as u32)
            .collect()
    } else {
        Vec::new()
    };
    result.sort_unstable();
    result.dedup();
    Ok(result)
}

#[derive(Default)]
struct TransferState {
    current: Option<PathBuf>,
    pages: Vec<PathBuf>,
    failure: Option<String>,
    limit_reached: bool,
}
#[implement(IWiaTransferCallback)]
struct FileTransfer {
    directory: PathBuf,
    max_pages: u32,
    cancel: CancellationToken,
    state: Arc<Mutex<TransferState>>,
}
impl IWiaTransferCallback_Impl for FileTransfer_Impl {
    fn TransferCallback(
        &self,
        _flags: i32,
        params: *const WiaTransferParams,
    ) -> windows::core::Result<()> {
        // SAFETY: WIA guarantees a valid structure for the duration of this callback; guard null defensively.
        let params = unsafe { params.as_ref() }
            .ok_or_else(|| windows::core::Error::from_hresult(E_POINTER))?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| windows::core::Error::from_hresult(E_FAIL))?;
        if params.ulTransferredBytes > processing::MAX_FILE_BYTES {
            state.failure = Some("Scanner page exceeds 256 MB; lower resolution".into());
            return Err(windows::core::Error::from_hresult(E_ABORT));
        }
        if params.lMessage == WIA_TRANSFER_MSG_END_OF_STREAM as i32
            && let Some(path) = state.current.take()
        {
            // WIA may still hold an exclusive file handle here. Inspect bytes only
            // after Download returns and its stream references are released.
            state.pages.push(path);
        }
        if self.cancel.is_cancelled() {
            return Err(windows::core::Error::from_hresult(E_ABORT));
        }
        Ok(())
    }
    fn GetNextStream(
        &self,
        _flags: i32,
        _name: &BSTR,
        _full_name: &BSTR,
    ) -> windows::core::Result<IStream> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| windows::core::Error::from_hresult(E_FAIL))?;
        if self.cancel.is_cancelled() {
            return Err(windows::core::Error::from_hresult(E_ABORT));
        }
        if state.pages.len() >= self.max_pages as usize {
            state.limit_reached = true;
            return Err(windows::core::Error::from_hresult(E_ABORT));
        }
        let path = self.directory.join(format!("{}.bmp", uuid::Uuid::new_v4()));
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|_| windows::core::Error::from_hresult(E_FAIL))?;
        // SAFETY: native file stream opens only the new file owned by this batch; WIA owns its returned reference.
        let stream = unsafe {
            SHCreateStreamOnFileEx(
                &HSTRING::from(path.as_os_str()),
                (STGM_READWRITE | STGM_SHARE_EXCLUSIVE).0,
                0,
                false,
                None::<&IStream>,
            )
        }?;
        state.current = Some(path);
        Ok(stream)
    }
}
fn acquire_direct(
    request: &ScanRequest,
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(Progress),
) -> Result<ScanBatch> {
    if request
        .settings
        .source
        .as_deref()
        .is_some_and(|source| !["Flatbed", "Feeder", "Duplex"].contains(&source))
    {
        return Err(ScanError::Unsupported(
            "Unknown WIA acquisition source".into(),
        ));
    }
    let _apartment = Apartment::start()?;
    let id = request
        .device_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ScanError::InvalidRequest("Select a scanner first".into()))?;
    progress(Progress::Connecting);
    // SAFETY: all COM calls below are confined to this initialized apartment.
    unsafe {
        let root = manager()?.CreateDevice(0, &BSTR::from(id)).map_err(error)?;
        let feeder = request
            .settings
            .source
            .as_deref()
            .is_some_and(|s| s == "Feeder" || s == "Duplex");
        let category = if feeder {
            WIA_CATEGORY_FEEDER
        } else {
            WIA_CATEGORY_FLATBED
        };
        let item = children(&root)?
            .into_iter()
            .find(|item| item.GetItemCategory().ok() == Some(category))
            .ok_or_else(|| {
                ScanError::Unsupported(
                    "Selected source is unavailable; refresh scanners or use the scanner interface"
                        .into(),
                )
            })?;
        let properties: IWiaPropertyStorage = item.cast().map_err(error)?;
        let dpi = request.settings.dpi.unwrap_or(300) as i32;
        set_integer(&properties, WIA_IPS_XRES, dpi)?;
        set_integer(&properties, WIA_IPS_YRES, dpi)?;
        let actual = integer(&properties, WIA_IPS_XRES)?;
        if actual != dpi || integer(&properties, WIA_IPS_YRES)? != dpi {
            return Err(ScanError::Unsupported(
                "Driver did not accept the requested resolution".into(),
            ));
        }
        set_integer(
            &properties,
            WIA_IPA_DATATYPE,
            if request.settings.color == Some(ColorMode::Gray) {
                2
            } else {
                3
            },
        )?;
        set_integer(&properties, WIA_IPS_XPOS, 0)?;
        set_integer(&properties, WIA_IPS_YPOS, 0)?;
        let (width, height) = request.settings.paper.unwrap_or(Paper::Letter).inches();
        set_integer(
            &properties,
            WIA_IPS_XEXTENT,
            (width * f64::from(dpi)).round() as i32,
        )?;
        set_integer(
            &properties,
            WIA_IPS_YEXTENT,
            (height * f64::from(dpi)).round() as i32,
        )?;
        if feeder {
            set_integer(
                &properties,
                WIA_IPS_DOCUMENT_HANDLING_SELECT,
                if request.settings.duplex == Some(true) {
                    DUPLEX as i32
                } else {
                    FRONT_ONLY as i32
                },
            )?;
            set_integer(&properties, WIA_IPS_PAGES, 0)?;
        }
        let transfer: IWiaTransfer = item.cast().map_err(error)?;
        let state = Arc::new(Mutex::new(TransferState::default()));
        let callback: IWiaTransferCallback = FileTransfer {
            directory: request.output_dir.canonicalize()?,
            max_pages: request.settings.max_pages,
            cancel: cancel.clone(),
            state: state.clone(),
        }
        .into();
        progress(Progress::Acquiring);
        let outcome = transfer.Download(0, &callback);
        drop(callback);
        drop(transfer);
        let mut state = state
            .lock()
            .map_err(|_| ScanError::Backend("Scanner callback failed".into()))?;
        let mut valid = Vec::new();
        for path in &state.pages {
            if fs::metadata(path)
                .is_ok_and(|info| info.len() > 0 && info.len() <= processing::MAX_FILE_BYTES)
            {
                valid.push(path.clone());
            }
        }
        if valid.len() != state.pages.len() {
            state.failure = Some(
                "Scanner returned an empty or oversized stream; complete pages were retained"
                    .into(),
            );
        }
        state.pages = valid;
        for path in &state.pages {
            progress(Progress::PageReady(path.clone()));
        }
        let warning = state.failure.clone().or_else(|| {
            outcome
                .err()
                .filter(|e| !state.limit_reached && e.code() != WIA_ERROR_PAPER_EMPTY)
                .map(|e| error(e).to_string())
        });
        if state.pages.is_empty() {
            return Err(if cancel.is_cancelled() {
                ScanError::Cancelled
            } else {
                ScanError::Backend(
                    warning
                        .unwrap_or_else(|| "No pages received; load the scanner and retry".into()),
                )
            });
        }
        Ok(ScanBatch {
            pages: state.pages.clone(),
            cancelled: cancel.is_cancelled(),
            warning,
            dpi: Some(dpi as u32),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_stream_callback_preserves_completed_page_and_enforces_limit() {
        let _apartment = Apartment::start().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let state = Arc::new(Mutex::new(TransferState::default()));
        let callback: IWiaTransferCallback = FileTransfer {
            directory: temp.path().into(),
            max_pages: 1,
            cancel: CancellationToken::default(),
            state: state.clone(),
        }
        .into();
        // SAFETY: native callback and stream are confined to this initialized test apartment.
        unsafe {
            let stream = callback
                .GetNextStream(0, &BSTR::new(), &BSTR::new())
                .unwrap();
            let bytes = b"complete test stream";
            stream
                .Write(bytes.as_ptr().cast(), bytes.len() as u32, None)
                .ok()
                .unwrap();
            callback
                .TransferCallback(
                    0,
                    &WiaTransferParams {
                        lMessage: WIA_TRANSFER_MSG_END_OF_STREAM as i32,
                        ..Default::default()
                    },
                )
                .unwrap();
            drop(stream);
            assert!(
                callback
                    .GetNextStream(0, &BSTR::new(), &BSTR::new())
                    .is_err()
            );
        }
        let state = state.lock().unwrap();
        assert_eq!(state.pages.len(), 1);
        assert!(state.limit_reached);
        assert_eq!(fs::read(&state.pages[0]).unwrap(), b"complete test stream");
    }
}
