//! Raw FFI types matching the PCO SDK C structs.
//!
//! All structs use `#[repr(C, packed)]` to match the SDK's `#pragma pack(1)`.

/// PCO_Description — camera hardware capabilities.
/// ~436 bytes, packed.
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct PcoDescription {
    pub w_size: u16,
    pub w_sensor_type_desc: u16,
    pub w_sensor_sub_type_desc: u16,
    pub w_max_horz_res_std_desc: u16,
    pub w_max_vert_res_std_desc: u16,
    pub w_max_horz_res_ext_desc: u16,
    pub w_max_vert_res_ext_desc: u16,
    pub w_dyn_res_desc: u16,
    pub w_max_bin_horz_desc: u16,
    pub w_bin_horz_stepping_desc: u16,
    pub w_max_bin_vert_desc: u16,
    pub w_bin_vert_stepping_desc: u16,
    pub w_roi_hor_steps_desc: u16,
    pub w_roi_vert_steps_desc: u16,
    pub w_num_adcs_desc: u16,
    pub w_min_size_horz_desc: u16,
    pub dw_pixel_rate_desc: [u32; 4],
    pub zz_dw_dummy_pr: [u32; 20],
    pub w_conv_fact_desc: [u16; 4],
    pub s_cooling_setpoints: [i16; 10],
    pub zz_dw_dummy_cv: [u16; 8],
    pub w_soft_roi_hor_steps_desc: u16,
    pub w_soft_roi_vert_steps_desc: u16,
    pub w_ir_desc: u16,
    pub w_min_size_vert_desc: u16,
    pub dw_min_delay_desc: u32,
    pub dw_max_delay_desc: u32,
    pub dw_min_delay_step_desc: u32,
    pub dw_min_expos_desc: u32,
    pub dw_max_expos_desc: u32,
    pub dw_min_expos_step_desc: u32,
    pub dw_min_delay_ir_desc: u32,
    pub dw_max_delay_ir_desc: u32,
    pub dw_min_expos_ir_desc: u32,
    pub dw_max_expos_ir_desc: u32,
    pub w_time_table_desc: u16,
    pub w_double_image_desc: u16,
    pub s_min_cool_set_desc: i16,
    pub s_max_cool_set_desc: i16,
    pub s_default_cool_set_desc: i16,
    pub w_power_down_mode_desc: u16,
    pub w_offset_regulation_desc: u16,
    pub w_color_pattern_desc: u16,
    pub w_pattern_type_desc: u16,
    pub w_dummy1: u16,
    pub w_dummy2: u16,
    pub w_num_cooling_setpoints: u16,
    pub dw_general_caps_desc1: u32,
    pub dw_general_caps_desc2: u32,
    pub dw_ext_sync_frequency: [u32; 4],
    pub dw_general_caps_desc3: u32,
    pub dw_general_caps_desc4: u32,
    pub zz_dw_dummy: [u32; 40],
}

impl PcoDescription {
    pub fn zeroed() -> Self {
        // SAFETY: All fields are integer types, zero is valid.
        unsafe { std::mem::zeroed() }
    }
}

/// PCO_CameraType — camera type, serial number, interface info.
///
/// From PCO SDK (Python ctypes in sdk.py lines 223-237):
///   wSize:               u16     (2)
///   wCamType:            u16     (2)
///   wCamSubType:         u16     (2)
///   ZZwAlignDummy1:      u16     (2)
///   dwSerialNumber:      u32     (4)
///   dwHWVersion:         u32     (4)
///   dwFWVersion:         u32     (4)
///   wInterfaceType:      u16     (2)
///   strHardwareVersion:  PCO_HW_Vers  (2 + 10 * (16+2+2+2+20*2)) = 622
///   strFirmwareVersion:  PCO_FW_Vers  (2 + 10 * (16+1+1+2+22*2)) = 642
///   ZZwDummy:            u16[39] (78)
///
/// Total: 22 + 622 + 642 + 78 = 1364 bytes.
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct PcoCameraType {
    pub w_size: u16,
    pub w_cam_type: u16,
    pub w_cam_sub_type: u16,
    pub zz_w_align_dummy1: u16,
    pub dw_serial_number: u32,
    pub dw_hw_version: u32,
    pub dw_fw_version: u32,
    pub w_interface_type: u16,
    // strHardwareVersion (622) + strFirmwareVersion (642) + ZZwDummy[39] (78)
    pub zz_tail: [u8; 1342],
}

impl PcoCameraType {
    /// Create a zeroed struct with wSize pre-set to the correct value.
    pub fn new() -> Self {
        let mut s: Self = unsafe { std::mem::zeroed() };
        s.w_size = std::mem::size_of::<Self>() as u16;
        s
    }
}

/// PCO_OpenStruct — extended open parameters for PCO_OpenCameraEx.
///
/// From PCO SDK (Python ctypes in sdk.py):
///   wSize:                 u16     (2)
///   wInterfaceType:        u16     (2)  — 0xFFFF = any interface
///   wCameraNumber:         u16     (2)
///   wCameraNumAtInterface: u16     (2)
///   wOpenFlags:            u16[10] (20) — [0] bit 0 = suppress dialog
///   dwOpenFlags:           u32[5]  (20)
///   wOpenPtr:              *[6]    (48 on x64)
///   zzwDummy:              u16[8]  (16)
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct PcoOpenStruct {
    pub w_size: u16,
    pub w_interface_type: u16,
    pub w_camera_number: u16,
    pub w_camera_num_at_interface: u16,
    pub w_open_flags: [u16; 10],
    pub dw_open_flags: [u32; 5],
    pub w_open_ptr: [usize; 6],
    pub zz_w_dummy: [u16; 8],
}

impl PcoOpenStruct {
    /// Create a zeroed struct configured for silent open of a given camera number.
    pub fn new_silent(camera_number: u16) -> Self {
        let mut s: Self = unsafe { std::mem::zeroed() };
        s.w_size = std::mem::size_of::<Self>() as u16;
        s.w_interface_type = 0xFFFF; // Any interface
        s.w_camera_number = camera_number;
        s.w_open_flags[0] = 0x0001; // Suppress dialog/progress bar
        s
    }
}

/// PCO_TIMESTAMP_STRUCT — hardware timestamp from recorder.
/// 22 bytes, packed.
#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct PcoTimestamp {
    pub w_size: u16,
    pub dw_img_counter: u32,
    pub w_year: u16,
    pub w_month: u16,
    pub w_day: u16,
    pub w_hour: u16,
    pub w_minute: u16,
    pub w_second: u16,
    pub dw_microseconds: u32,
}

impl PcoTimestamp {
    pub fn zeroed() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

/// PCO_METADATA_STRUCT — image metadata from recorder.
/// ~80 bytes, packed.
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct PcoMetadata {
    pub w_size: u16,
    pub w_version: u16,
    pub b_image_counter_bcd: [u8; 4],
    pub b_image_time_us_bcd: [u8; 3],
    pub b_image_time_sec_bcd: u8,
    pub b_image_time_min_bcd: u8,
    pub b_image_time_hour_bcd: u8,
    pub b_image_time_day_bcd: u8,
    pub b_image_time_mon_bcd: u8,
    pub b_image_time_year_bcd: u8,
    pub b_image_time_status: u8,
    pub w_exposure_time_base: u16,
    pub dw_exposure_time: u32,
    pub dw_framerate_millihz: u32,
    pub s_sensor_temperature: i16,
    pub w_image_size_x: u16,
    pub w_image_size_y: u16,
    pub b_binning_x: u8,
    pub b_binning_y: u8,
    pub dw_sensor_readout_frequency: u32,
    pub w_sensor_conv_factor: u16,
    pub dw_camera_serial_no: u32,
    pub w_camera_type: u16,
    pub b_bit_resolution: u8,
    pub b_sync_status: u8,
    pub w_dark_offset: u16,
    pub b_trigger_mode: u8,
    pub b_double_image_mode: u8,
    pub b_camera_sync_mode: u8,
    pub b_image_type: u8,
    pub w_color_pattern: u16,
    pub w_camera_subtype: u16,
    pub dw_event_number: u32,
    pub w_image_size_x_offset: u16,
    pub w_image_size_y_offset: u16,
}

impl PcoMetadata {
    pub fn zeroed() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
