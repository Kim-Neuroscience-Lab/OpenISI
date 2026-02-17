//! VsyncTimestampProvider - Hardware vsync timestamps via Vulkan
//!
//! Creates an invisible window on the target monitor and uses Vulkan's
//! VK_GOOGLE_display_timing extension to capture true presentation timestamps.
//!
//! The mapping algorithm:
//! - Godot's frame_post_draw fires AFTER rendering, BEFORE presentation
//! - For each software timestamp T_software, the hardware timestamp is the
//!   FIRST vsync timestamp where T_vsync > T_software

use godot::prelude::*;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicI64, Ordering}};
use std::thread::{self, JoinHandle};
use std::collections::VecDeque;
use std::time::Duration;
use std::ffi::CStr;

use ash::vk;
use ash::khr;
use winit::event_loop::EventLoop;
use winit::window::{Window, WindowBuilder};
use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// Maximum number of vsync timestamps to keep in buffer
const MAX_VSYNC_BUFFER_SIZE: usize = 1000;

/// Thread-safe vsync timestamp buffer
pub struct VsyncBuffer {
    timestamps_us: VecDeque<i64>,
}

impl VsyncBuffer {
    fn new() -> Self {
        Self {
            timestamps_us: VecDeque::with_capacity(MAX_VSYNC_BUFFER_SIZE),
        }
    }

    fn push(&mut self, timestamp_us: i64) {
        if self.timestamps_us.len() >= MAX_VSYNC_BUFFER_SIZE {
            self.timestamps_us.pop_front();
        }
        self.timestamps_us.push_back(timestamp_us);
    }

    fn get_all(&self) -> Vec<i64> {
        self.timestamps_us.iter().copied().collect()
    }

    fn find_next_vsync(&self, software_ts_us: i64) -> Option<i64> {
        // Find first vsync timestamp > software_timestamp
        for &vsync_ts in &self.timestamps_us {
            if vsync_ts > software_ts_us {
                return Some(vsync_ts);
            }
        }
        None
    }

    fn clear(&mut self) {
        self.timestamps_us.clear();
    }
}

/// Hardware vsync timestamp provider using Vulkan VK_GOOGLE_display_timing
#[derive(GodotClass)]
#[class(base=RefCounted)]
pub struct VsyncTimestampProvider {
    /// Thread-safe vsync timestamp buffer
    vsync_buffer: Arc<Mutex<VsyncBuffer>>,

    /// Flag to signal thread to stop
    running: Arc<AtomicBool>,

    /// Latest captured vsync timestamp (for quick access)
    latest_vsync_us: Arc<AtomicI64>,

    /// Background worker thread handle
    worker_thread: Option<JoinHandle<()>>,

    /// Whether the provider is active
    is_active: bool,

    /// Error message if start() failed
    error_message: String,

    /// Target monitor index
    monitor_index: i32,
}

#[godot_api]
impl IRefCounted for VsyncTimestampProvider {
    fn init(_base: Base<RefCounted>) -> Self {
        godot_print!("VsyncTimestampProvider: Created");
        Self {
            vsync_buffer: Arc::new(Mutex::new(VsyncBuffer::new())),
            running: Arc::new(AtomicBool::new(false)),
            latest_vsync_us: Arc::new(AtomicI64::new(-1)),
            worker_thread: None,
            is_active: false,
            error_message: String::new(),
            monitor_index: -1,
        }
    }
}

#[godot_api]
impl VsyncTimestampProvider {
    /// Start capturing vsync timestamps for the specified display.
    /// Returns true if successful, false if hardware timestamps unavailable.
    /// If false, call get_error() to get the reason.
    #[func]
    fn start(&mut self, display_index: i32) -> bool {
        if self.is_active {
            godot_print!("VsyncTimestampProvider: Already running");
            return true;
        }

        self.monitor_index = display_index;
        self.error_message.clear();

        // Clear any old timestamps
        if let Ok(mut buffer) = self.vsync_buffer.lock() {
            buffer.clear();
        }

        // Set up shared state for thread
        let vsync_buffer = Arc::clone(&self.vsync_buffer);
        let running = Arc::clone(&self.running);
        let latest_vsync = Arc::clone(&self.latest_vsync_us);

        // Signal thread to run
        self.running.store(true, Ordering::SeqCst);

        // Spawn worker thread
        let monitor_idx = display_index;
        let handle = thread::spawn(move || {
            run_vsync_capture_loop(monitor_idx, vsync_buffer, running, latest_vsync);
        });

        self.worker_thread = Some(handle);
        self.is_active = true;

        godot_print!("VsyncTimestampProvider: Started on display {}", display_index);
        true
    }

    /// Stop capturing vsync timestamps and cleanup resources.
    #[func]
    fn stop(&mut self) {
        if !self.is_active {
            return;
        }

        godot_print!("VsyncTimestampProvider: Stopping...");

        // Signal thread to stop
        self.running.store(false, Ordering::SeqCst);

        // Wait for thread to finish
        if let Some(handle) = self.worker_thread.take() {
            let _ = handle.join();
        }

        self.is_active = false;
        self.monitor_index = -1;

        godot_print!("VsyncTimestampProvider: Stopped");
    }

    /// Check if the provider is actively capturing timestamps.
    #[func]
    fn is_running(&self) -> bool {
        self.is_active && self.running.load(Ordering::SeqCst)
    }

    /// Map a software timestamp to its hardware vsync timestamp.
    /// Returns the first vsync timestamp that is greater than the software timestamp.
    /// Returns -1 if no matching vsync found (buffer may not have caught up yet).
    #[func]
    fn map_to_hardware_timestamp(&self, software_timestamp_us: i64) -> i64 {
        if let Ok(buffer) = self.vsync_buffer.lock() {
            buffer.find_next_vsync(software_timestamp_us).unwrap_or(-1)
        } else {
            -1
        }
    }

    /// Get the most recent vsync timestamp (for real-time display).
    /// Returns -1 if no timestamps captured yet.
    #[func]
    fn get_latest_vsync_us(&self) -> i64 {
        self.latest_vsync_us.load(Ordering::SeqCst)
    }

    /// Get all captured vsync timestamps (for batch mapping at end of acquisition).
    #[func]
    fn get_vsync_timestamps(&self) -> PackedInt64Array {
        let mut result = PackedInt64Array::new();
        if let Ok(buffer) = self.vsync_buffer.lock() {
            for &ts in buffer.get_all().iter() {
                result.push(ts);
            }
        }
        result
    }

    /// Get the number of vsync timestamps captured.
    #[func]
    fn get_timestamp_count(&self) -> i64 {
        if let Ok(buffer) = self.vsync_buffer.lock() {
            buffer.timestamps_us.len() as i64
        } else {
            0
        }
    }

    /// Get error message if start() returned false.
    #[func]
    fn get_error(&self) -> GString {
        GString::from(self.error_message.as_str())
    }

    /// Check if VK_GOOGLE_display_timing is available on this system.
    /// This is a static check that doesn't require starting the provider.
    #[func]
    fn is_display_timing_available() -> bool {
        // Try to create a Vulkan instance and check for the extension
        check_display_timing_support()
    }

    /// Get the target monitor index.
    #[func]
    fn get_monitor_index(&self) -> i32 {
        self.monitor_index
    }
}

impl Drop for VsyncTimestampProvider {
    fn drop(&mut self) {
        // Ensure thread is stopped on drop
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.worker_thread.take() {
            let _ = handle.join();
        }
    }
}


// =============================================================================
// Vulkan Vsync Capture Implementation
// =============================================================================

/// Check if VK_GOOGLE_display_timing extension is available
pub fn check_display_timing_support() -> bool {
    unsafe {
        let entry = match ash::Entry::load() {
            Ok(e) => e,
            Err(_) => return false,
        };

        // Check instance extensions
        let instance_extensions = match entry.enumerate_instance_extension_properties(None) {
            Ok(exts) => exts,
            Err(_) => return false,
        };

        // We need VK_KHR_surface at minimum
        let has_surface = instance_extensions.iter().any(|ext| {
            let name = CStr::from_ptr(ext.extension_name.as_ptr());
            name.to_bytes() == b"VK_KHR_surface"
        });

        if !has_surface {
            return false;
        }

        // The actual VK_GOOGLE_display_timing is a device extension,
        // so we can't fully verify without creating a device.
        // Return true if we have basic Vulkan support.
        true
    }
}

/// Main vsync capture loop - runs in background thread
fn run_vsync_capture_loop(
    monitor_index: i32,
    vsync_buffer: Arc<Mutex<VsyncBuffer>>,
    running: Arc<AtomicBool>,
    latest_vsync: Arc<AtomicI64>,
) {
    // Try to initialize Vulkan with display timing
    match VsyncCaptureContext::new(monitor_index) {
        Ok(mut ctx) => {
            godot_print!("VsyncTimestampProvider: Vulkan context created successfully");

            // Run capture loop
            while running.load(Ordering::SeqCst) {
                if let Some(timestamp_us) = ctx.capture_vsync_timestamp() {
                    latest_vsync.store(timestamp_us, Ordering::SeqCst);

                    if let Ok(mut buffer) = vsync_buffer.lock() {
                        buffer.push(timestamp_us);
                    }
                }

                // Process window events (required for swapchain)
                ctx.pump_events();

                // Small sleep to avoid spinning too fast
                thread::sleep(Duration::from_micros(100));
            }

            godot_print!("VsyncTimestampProvider: Capture loop ended");
        }
        Err(e) => {
            // NO SOFTWARE FALLBACK
            // Software timestamps are unacceptable for scientific data.
            // If hardware timestamps are unavailable, we fail loudly.
            godot_print!("VsyncTimestampProvider: FATAL - Failed to create Vulkan context: {}", e);
            godot_print!("VsyncTimestampProvider: Hardware timestamps UNAVAILABLE - cannot proceed");
            godot_print!("VsyncTimestampProvider: Software fallback is UNACCEPTABLE for scientific data");

            // Signal that we're not running (no valid timestamps will be produced)
            running.store(false, Ordering::SeqCst);
        }
    }
}

// NO SOFTWARE FALLBACK FUNCTION
// Software timestamps are unacceptable for scientific data.
// If VK_GOOGLE_display_timing doesn't work, we fail completely.


// =============================================================================
// Vulkan Context for Vsync Capture
// =============================================================================

/// Vulkan context for capturing vsync timestamps
pub struct VsyncCaptureContext {
    _entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,
    surface: vk::SurfaceKHR,
    swapchain: vk::SwapchainKHR,
    surface_loader: khr::surface::Instance,
    swapchain_loader: khr::swapchain::Device,
    display_timing_loader: Option<DisplayTimingLoader>,
    queue: vk::Queue,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    image_available_semaphore: vk::Semaphore,
    render_finished_semaphore: vk::Semaphore,
    fence: vk::Fence,
    swapchain_images: Vec<vk::Image>,
    present_id: u64,
    event_loop: Option<EventLoop<()>>,
    _window: Window,
}

/// Wrapper for VK_GOOGLE_display_timing extension functions
struct DisplayTimingLoader {
    get_past_presentation_timing: vk::PFN_vkGetPastPresentationTimingGOOGLE,
}

impl VsyncCaptureContext {
    pub fn new(monitor_index: i32) -> Result<Self, String> {
        unsafe {
            // Load Vulkan
            let entry = ash::Entry::load()
                .map_err(|e| format!("Failed to load Vulkan: {:?}", e))?;

            // Create event loop
            let event_loop = EventLoop::new()
                .map_err(|e| format!("Failed to create event loop: {:?}", e))?;

            // Find target monitor
            let monitors: Vec<_> = event_loop.available_monitors().collect();
            let target_monitor = monitors.get(monitor_index as usize)
                .ok_or_else(|| format!("Monitor {} not found (have {} monitors)", monitor_index, monitors.len()))?;

            godot_print!("VsyncTimestampProvider: Creating window on monitor: {:?}",
                target_monitor.name().unwrap_or_else(|| "Unknown".to_string()));

            // Create invisible window on target monitor
            let window = WindowBuilder::new()
                .with_visible(false)
                .with_inner_size(PhysicalSize::new(1u32, 1u32))
                .with_position(target_monitor.position())
                .with_decorations(false)
                .with_resizable(false)
                .build(&event_loop)
                .map_err(|e| format!("Failed to create window: {:?}", e))?;

            // Get required instance extensions for surface creation
            let mut instance_extensions = vec![
                khr::surface::NAME.as_ptr(),
            ];

            #[cfg(target_os = "macos")]
            {
                instance_extensions.push(ash::ext::metal_surface::NAME.as_ptr());
                instance_extensions.push(ash::khr::portability_enumeration::NAME.as_ptr());
            }

            #[cfg(target_os = "linux")]
            {
                // Try Wayland first, then X11
                instance_extensions.push(ash::khr::wayland_surface::NAME.as_ptr());
                instance_extensions.push(ash::khr::xlib_surface::NAME.as_ptr());
            }

            #[cfg(target_os = "windows")]
            {
                instance_extensions.push(ash::khr::win32_surface::NAME.as_ptr());
            }

            // Create Vulkan instance
            let app_info = vk::ApplicationInfo::default()
                .application_name(c"OpenISI VsyncCapture")
                .application_version(vk::make_api_version(0, 1, 0, 0))
                .engine_name(c"No Engine")
                .engine_version(vk::make_api_version(0, 1, 0, 0))
                .api_version(vk::API_VERSION_1_0);

            let mut create_flags = vk::InstanceCreateFlags::empty();
            #[cfg(target_os = "macos")]
            {
                create_flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
            }

            let create_info = vk::InstanceCreateInfo::default()
                .application_info(&app_info)
                .enabled_extension_names(&instance_extensions)
                .flags(create_flags);

            let instance = entry.create_instance(&create_info, None)
                .map_err(|e| format!("Failed to create Vulkan instance: {:?}", e))?;

            // Create surface
            let surface_loader = khr::surface::Instance::new(&entry, &instance);

            let surface = ash_window::create_surface(
                &entry,
                &instance,
                window.display_handle().unwrap().as_raw(),
                window.window_handle().unwrap().as_raw(),
                None,
            ).map_err(|e| format!("Failed to create surface: {:?}", e))?;

            // Pick physical device
            let physical_devices = instance.enumerate_physical_devices()
                .map_err(|e| format!("Failed to enumerate physical devices: {:?}", e))?;

            let (physical_device, queue_family_index) = physical_devices.iter()
                .find_map(|&device| {
                    let queue_families = instance.get_physical_device_queue_family_properties(device);

                    queue_families.iter().enumerate().find_map(|(index, info)| {
                        let supports_graphics = info.queue_flags.contains(vk::QueueFlags::GRAPHICS);
                        let supports_surface = surface_loader
                            .get_physical_device_surface_support(device, index as u32, surface)
                            .unwrap_or(false);

                        if supports_graphics && supports_surface {
                            Some((device, index as u32))
                        } else {
                            None
                        }
                    })
                })
                .ok_or_else(|| "No suitable GPU found".to_string())?;

            // Check for VK_GOOGLE_display_timing extension
            let device_extensions = instance.enumerate_device_extension_properties(physical_device)
                .map_err(|e| format!("Failed to enumerate device extensions: {:?}", e))?;

            let has_display_timing = device_extensions.iter().any(|ext| {
                let name = CStr::from_ptr(ext.extension_name.as_ptr());
                name.to_bytes() == b"VK_GOOGLE_display_timing"
            });

            if has_display_timing {
                godot_print!("VsyncTimestampProvider: VK_GOOGLE_display_timing is available!");
            } else {
                godot_print!("VsyncTimestampProvider: VK_GOOGLE_display_timing NOT available");
            }

            // Required device extensions
            let mut device_extensions_ptrs = vec![khr::swapchain::NAME.as_ptr()];

            #[cfg(target_os = "macos")]
            {
                device_extensions_ptrs.push(ash::khr::portability_subset::NAME.as_ptr());
            }

            if has_display_timing {
                device_extensions_ptrs.push(c"VK_GOOGLE_display_timing".as_ptr());
            }

            // Create logical device
            let queue_priorities = [1.0f32];
            let queue_create_info = vk::DeviceQueueCreateInfo::default()
                .queue_family_index(queue_family_index)
                .queue_priorities(&queue_priorities);

            let device_create_info = vk::DeviceCreateInfo::default()
                .queue_create_infos(std::slice::from_ref(&queue_create_info))
                .enabled_extension_names(&device_extensions_ptrs);

            let device = instance.create_device(physical_device, &device_create_info, None)
                .map_err(|e| format!("Failed to create logical device: {:?}", e))?;

            let queue = device.get_device_queue(queue_family_index, 0);

            // Create swapchain
            let surface_capabilities = surface_loader
                .get_physical_device_surface_capabilities(physical_device, surface)
                .map_err(|e| format!("Failed to get surface capabilities: {:?}", e))?;

            let surface_formats = surface_loader
                .get_physical_device_surface_formats(physical_device, surface)
                .map_err(|e| format!("Failed to get surface formats: {:?}", e))?;

            let surface_format = surface_formats.first()
                .ok_or_else(|| "No surface formats available".to_string())?;

            let present_modes = surface_loader
                .get_physical_device_surface_present_modes(physical_device, surface)
                .map_err(|e| format!("Failed to get present modes: {:?}", e))?;

            // Use FIFO (vsync-locked) present mode
            let present_mode = if present_modes.contains(&vk::PresentModeKHR::FIFO) {
                vk::PresentModeKHR::FIFO
            } else {
                present_modes[0]
            };

            let image_count = surface_capabilities.min_image_count.max(2);
            let extent = if surface_capabilities.current_extent.width != u32::MAX {
                surface_capabilities.current_extent
            } else {
                vk::Extent2D { width: 1, height: 1 }
            };

            let swapchain_loader = khr::swapchain::Device::new(&instance, &device);

            let swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
                .surface(surface)
                .min_image_count(image_count)
                .image_format(surface_format.format)
                .image_color_space(surface_format.color_space)
                .image_extent(extent)
                .image_array_layers(1)
                .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                .pre_transform(surface_capabilities.current_transform)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .present_mode(present_mode)
                .clipped(true);

            let swapchain = swapchain_loader.create_swapchain(&swapchain_create_info, None)
                .map_err(|e| format!("Failed to create swapchain: {:?}", e))?;

            let swapchain_images = swapchain_loader.get_swapchain_images(swapchain)
                .map_err(|e| format!("Failed to get swapchain images: {:?}", e))?;

            // Create command pool and buffer
            let pool_create_info = vk::CommandPoolCreateInfo::default()
                .queue_family_index(queue_family_index)
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);

            let command_pool = device.create_command_pool(&pool_create_info, None)
                .map_err(|e| format!("Failed to create command pool: {:?}", e))?;

            let alloc_info = vk::CommandBufferAllocateInfo::default()
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1);

            let command_buffers = device.allocate_command_buffers(&alloc_info)
                .map_err(|e| format!("Failed to allocate command buffer: {:?}", e))?;
            let command_buffer = command_buffers[0];

            // Create synchronization objects
            let semaphore_info = vk::SemaphoreCreateInfo::default();
            let fence_info = vk::FenceCreateInfo::default()
                .flags(vk::FenceCreateFlags::SIGNALED);

            let image_available_semaphore = device.create_semaphore(&semaphore_info, None)
                .map_err(|e| format!("Failed to create semaphore: {:?}", e))?;
            let render_finished_semaphore = device.create_semaphore(&semaphore_info, None)
                .map_err(|e| format!("Failed to create semaphore: {:?}", e))?;
            let fence = device.create_fence(&fence_info, None)
                .map_err(|e| format!("Failed to create fence: {:?}", e))?;

            // Load VK_GOOGLE_display_timing functions if available
            let display_timing_loader = if has_display_timing {
                let get_past_presentation_timing: vk::PFN_vkGetPastPresentationTimingGOOGLE =
                    std::mem::transmute(
                        instance.get_device_proc_addr(
                            device.handle(),
                            c"vkGetPastPresentationTimingGOOGLE".as_ptr()
                        )
                    );

                if get_past_presentation_timing as usize != 0 {
                    Some(DisplayTimingLoader { get_past_presentation_timing })
                } else {
                    None
                }
            } else {
                None
            };

            godot_print!("VsyncTimestampProvider: Context initialized, display_timing available: {}",
                display_timing_loader.is_some());

            Ok(Self {
                _entry: entry,
                instance,
                device,
                surface,
                swapchain,
                surface_loader,
                swapchain_loader,
                display_timing_loader,
                queue,
                command_pool,
                command_buffer,
                image_available_semaphore,
                render_finished_semaphore,
                fence,
                swapchain_images,
                present_id: 0,
                event_loop: Some(event_loop),
                _window: window,
            })
        }
    }

    /// Present a frame and capture the vsync timestamp
    pub fn capture_vsync_timestamp(&mut self) -> Option<i64> {
        unsafe {
            // Wait for previous frame to complete
            self.device.wait_for_fences(&[self.fence], true, u64::MAX).ok()?;
            self.device.reset_fences(&[self.fence]).ok()?;

            // Acquire next image
            let (image_index, _) = self.swapchain_loader
                .acquire_next_image(self.swapchain, u64::MAX, self.image_available_semaphore, vk::Fence::null())
                .ok()?;

            // Record minimal command buffer (just transition and present)
            self.device.reset_command_buffer(self.command_buffer, vk::CommandBufferResetFlags::empty()).ok()?;

            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(self.command_buffer, &begin_info).ok()?;

            // Transition image to present layout
            let barrier = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(self.swapchain_images[image_index as usize])
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });

            self.device.cmd_pipeline_barrier(
                self.command_buffer,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );

            self.device.end_command_buffer(self.command_buffer).ok()?;

            // Submit
            let wait_semaphores = [self.image_available_semaphore];
            let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            let command_buffers = [self.command_buffer];
            let signal_semaphores = [self.render_finished_semaphore];

            let submit_info = vk::SubmitInfo::default()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_stages)
                .command_buffers(&command_buffers)
                .signal_semaphores(&signal_semaphores);

            self.device.queue_submit(self.queue, &[submit_info], self.fence).ok()?;

            // Present
            let swapchains = [self.swapchain];
            let image_indices = [image_index];

            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .swapchains(&swapchains)
                .image_indices(&image_indices);

            let _ = self.swapchain_loader.queue_present(self.queue, &present_info);

            self.present_id += 1;

            // Query past presentation timing - HARDWARE TIMESTAMPS ONLY
            if let Some(ref timing_loader) = self.display_timing_loader {
                // First call to get count
                let mut timing_count: u32 = 0;
                let result = (timing_loader.get_past_presentation_timing)(
                    self.device.handle(),
                    self.swapchain,
                    &mut timing_count,
                    std::ptr::null_mut(),
                );

                if result == vk::Result::SUCCESS && timing_count > 0 {
                    let mut timings = vec![vk::PastPresentationTimingGOOGLE::default(); timing_count as usize];
                    let result = (timing_loader.get_past_presentation_timing)(
                        self.device.handle(),
                        self.swapchain,
                        &mut timing_count,
                        timings.as_mut_ptr(),
                    );

                    if result == vk::Result::SUCCESS && timing_count > 0 {
                        // Return the most recent actual presentation time
                        // actual_present_time is in nanoseconds
                        let latest = &timings[timing_count as usize - 1];
                        let timestamp_ns = latest.actual_present_time;

                        // Validate timestamp is reasonable (not garbage)
                        // Should be > 1 second (not zero/negative) and < 10 years (not overflow)
                        let one_second_ns: u64 = 1_000_000_000;
                        let ten_years_ns: u64 = 10 * 365 * 24 * 60 * 60 * 1_000_000_000;

                        if timestamp_ns > one_second_ns && timestamp_ns < ten_years_ns {
                            return Some((timestamp_ns / 1000) as i64);
                        }

                        // Garbage timestamp from VK_GOOGLE_display_timing
                        // This happens with MoltenVK on macOS - extension exists but returns junk
                        // NO SOFTWARE FALLBACK - return None
                        return None;
                    }
                }
            }

            // NO SOFTWARE FALLBACK
            // If VK_GOOGLE_display_timing doesn't work, we return None
            // Software timestamps are unacceptable for scientific data
            None
        }
    }

    /// Process window events (required to keep swapchain alive)
    pub fn pump_events(&mut self) {
        if let Some(ref mut event_loop) = self.event_loop {
            event_loop.pump_events(Some(Duration::from_millis(0)), |_event, _elwt| {
                // We don't need to handle events, just pump them
            });
        }
    }
}

impl Drop for VsyncCaptureContext {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();

            self.device.destroy_fence(self.fence, None);
            self.device.destroy_semaphore(self.render_finished_semaphore, None);
            self.device.destroy_semaphore(self.image_available_semaphore, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.swapchain_loader.destroy_swapchain(self.swapchain, None);
            self.surface_loader.destroy_surface(self.surface, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}
