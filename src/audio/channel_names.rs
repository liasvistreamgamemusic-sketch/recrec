//! Input channel names as the driver reports them (ASIO only).
//!
//! cpal does not expose ASIO channel names, so this asks the ASIO SDK directly. The SDK is
//! already linked in through cpal's `asio-sys`; only the declaration is repeated here.

/// Names of the first `channels` inputs of the loaded ASIO driver, or `None` when they
/// cannot be read (other hosts, or the driver refuses).
pub fn asio_input_names(host_name: &str, channels: u16) -> Option<Vec<String>> {
    if host_name != "ASIO" {
        return None;
    }
    imp::input_names(channels)
}

#[cfg(all(windows, feature = "asio"))]
mod imp {
    use std::ffi::c_long;

    const ASE_OK: c_long = 0;
    const NAME_LEN: usize = 32;

    /// `ASIOChannelInfo` from the SDK's `asio.h` (4-byte packed; every field is 4-aligned).
    #[repr(C)]
    struct ChannelInfo {
        channel: c_long,
        is_input: c_long,
        is_active: c_long,
        channel_group: c_long,
        sample_type: c_long,
        name: [u8; NAME_LEN],
    }

    unsafe extern "C" {
        // C++ symbol of `ASIOError ASIOGetChannelInfo(ASIOChannelInfo *)` (MSVC x64), exactly
        // as bindgen declares it inside asio-sys. The leading \u{1} stops further mangling.
        #[link_name = "\u{1}?ASIOGetChannelInfo@@YAJPEAUASIOChannelInfo@@@Z"]
        fn asio_get_channel_info(info: *mut ChannelInfo) -> c_long;
    }

    pub fn input_names(channels: u16) -> Option<Vec<String>> {
        (0..channels)
            .map(|channel| {
                let mut info = ChannelInfo {
                    channel: c_long::from(channel),
                    is_input: 1,
                    is_active: 0,
                    channel_group: 0,
                    sample_type: 0,
                    name: [0; NAME_LEN],
                };
                // SAFETY: `info` matches the SDK struct and outlives the call; the caller
                // holds an open stream, so a driver is loaded.
                let result = unsafe { asio_get_channel_info(&mut info) };
                if result != ASE_OK {
                    log::warn!("ASIOGetChannelInfo({channel}) failed: {result}");
                    return None;
                }
                let end = info.name.iter().position(|b| *b == 0).unwrap_or(NAME_LEN);
                let name = String::from_utf8_lossy(&info.name[..end]).trim().to_owned();
                (!name.is_empty()).then_some(name)
            })
            .collect()
    }
}

#[cfg(not(all(windows, feature = "asio")))]
mod imp {
    pub fn input_names(_channels: u16) -> Option<Vec<String>> {
        None
    }
}
