use std::mem::size_of;
use std::sync::Mutex;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
	CDS_FULLSCREEN, CDS_TYPE, ChangeDisplaySettingsExW, DEVMODEW, DISP_CHANGE_SUCCESSFUL,
	DM_DISPLAYFREQUENCY, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE, EnumDisplaySettingsW,
	GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromWindow,
};
use windows::core::PCWSTR;

static SWITCHED: Mutex<Option<[u16; 32]>> = Mutex::new(None);

/// Windows lists 23.976 Hz as 23, 59.94 Hz as 59 and so on.
fn actual(hz: u32) -> f64 {
	if matches!(hz, 23 | 29 | 47 | 59 | 71 | 119) {
		(hz + 1) as f64 * 1000.0 / 1001.0
	} else {
		hz as f64
	}
}

/// The rate that shows each frame for the same time: an exact multiple of the frame rate first, then one within 0.2 %;
/// the lowest multiple wins, not the highest rate.
fn pick(fps: f64, rates: &[u32]) -> Option<u32> {
	rates
		.iter()
		.filter_map(|&hz| {
			let actual = actual(hz);
			let multiple = (actual / fps).round();
			let error = (actual - multiple * fps).abs() / actual;
			(multiple >= 1.0 && error < 0.002).then_some((error >= 0.0005, multiple as u32, hz))
		})
		.min()
		.map(|(_, _, hz)| hz)
}

/// The display's rate if it matches the video, switched to or already set.
pub fn switch(window: HWND, fps: f64) -> windows::core::Result<Option<f64>> {
	unsafe {
		let mut info = MONITORINFOEXW::default();
		info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
		GetMonitorInfoW(
			MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
			&mut info as *mut _ as *mut MONITORINFO,
		)
		.ok()?;
		let device = PCWSTR(info.szDevice.as_ptr());
		let mut current = DEVMODEW {
			dmSize: size_of::<DEVMODEW>() as u16,
			..Default::default()
		};
		EnumDisplaySettingsW(device, ENUM_CURRENT_SETTINGS, &mut current).ok()?;
		let mut rates = Vec::new();
		let mut mode = DEVMODEW {
			dmSize: size_of::<DEVMODEW>() as u16,
			..Default::default()
		};
		for i in 0.. {
			if !EnumDisplaySettingsW(device, ENUM_DISPLAY_SETTINGS_MODE(i), &mut mode).as_bool() {
				break;
			}
			if (mode.dmPelsWidth, mode.dmPelsHeight, mode.dmBitsPerPel)
				== (
					current.dmPelsWidth,
					current.dmPelsHeight,
					current.dmBitsPerPel,
				) {
				rates.push(mode.dmDisplayFrequency);
			}
		}
		let Some(hz) = pick(fps, &rates) else {
			return Ok(None);
		};
		if hz == current.dmDisplayFrequency {
			return Ok(Some(actual(hz)));
		}
		current.dmDisplayFrequency = hz;
		current.dmFields = DM_DISPLAYFREQUENCY;
		if ChangeDisplaySettingsExW(device, Some(&current), None, CDS_FULLSCREEN, None)
			!= DISP_CHANGE_SUCCESSFUL
		{
			return Ok(None);
		}
		*SWITCHED.lock().unwrap() = Some(info.szDevice);
		Ok(Some(actual(hz)))
	}
}

/// Back to the mode saved in the registry, the one the user chose.
pub fn restore() {
	let Some(device) = SWITCHED.lock().unwrap().take() else {
		return;
	};
	unsafe { ChangeDisplaySettingsExW(PCWSTR(device.as_ptr()), None, None, CDS_TYPE(0), None) };
}

#[cfg(test)]
mod tests {
	use super::pick;

	#[test]
	fn picks_the_lowest_matching_multiple() {
		let rates = [23, 24, 50, 59, 60, 119, 120];
		assert_eq!(pick(24000.0 / 1001.0, &rates), Some(23));
		assert_eq!(pick(24.0, &rates), Some(24));
		assert_eq!(pick(25.0, &rates), Some(50));
		assert_eq!(pick(30000.0 / 1001.0, &rates), Some(59));
		assert_eq!(pick(24000.0 / 1001.0, &[24, 60, 120]), Some(24));
		assert_eq!(pick(25.0, &[60, 144]), None);
	}
}
