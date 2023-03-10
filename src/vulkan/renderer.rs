use ash::vk;
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};

use super::{window::VulkanWindow};
use super::surface::VulkanSurface;
use super::debug::VulkanDebug;
use super::physical_device::PhysicalDevice;
use super::queue::*;
use super::logical_device::LogicalDevice;
use super::swapchain::VulkanSwapchain;
use super::render_pass::RenderPass;
use super::pipeline::Pipeline;
use super::command_pools::Pools;
use super::game_object::GameObject;

use crate::utils::{align, any_as_u8_slice};

pub struct VulkanRenderer {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub is_framebuffer_resized: bool,
    pub debug: VulkanDebug,
    pub surface: VulkanSurface,
    pub physical_device: vk::PhysicalDevice,
    pub physical_device_properties: vk::PhysicalDeviceProperties,
    pub physical_device_features: vk::PhysicalDeviceFeatures,
    pub queue_families: QueueFamilies,
    pub queues: Queues,
    pub device: ash::Device,
    pub swapchain: VulkanSwapchain,
    pub renderpass: vk::RenderPass,
    pub pipeline: Pipeline,
    pub pools: Pools,
    pub command_buffers: Vec<vk::CommandBuffer>,
    pub allocator: std::mem::ManuallyDrop<Allocator>,
    pub game_objects: Vec<GameObject>
}

impl VulkanRenderer {
    pub fn new(window: &VulkanWindow) -> Result<Self, Box<dyn std::error::Error>> {
        let layer_names = vec!["VK_LAYER_KHRONOS_validation"]; 
        let entry = ash::Entry::linked();
        let instance = Self::create_instance(&entry, &layer_names, &window)
            .expect("Failed to initialize instance!");
        
        let debug = VulkanDebug::new(&entry, &instance)?;

        let surface = VulkanSurface::new(&window, &entry, &instance)?;

        let (physical_device, physical_device_properties, physical_device_features) = PhysicalDevice::pick_physical_device(&instance)
            .expect("No suitable physical device found!");

        let queue_families = QueueFamilies::new(&instance, physical_device, &surface)?;

        let (logical_device, queues) = LogicalDevice::new(&instance, physical_device, &queue_families, &layer_names)?;

        let mut swapchain = VulkanSwapchain::new(&instance, physical_device, &logical_device, &surface, &queue_families)?;

        let renderpass = RenderPass::init(&logical_device, swapchain.surface_format.format)?;

        swapchain.create_framebuffers(&logical_device, renderpass)?;

        let pipeline = Pipeline::new(&logical_device, &swapchain, &renderpass)?;

        let pools = Pools::new(&logical_device, &queue_families)?;

        let buffer_device_address = false;
        let allocator = Allocator::new(&AllocatorCreateDesc {
            instance: instance.clone(),
            device: logical_device.clone(),
            physical_device,
            debug_settings: Default::default(),
            buffer_device_address,
        }).expect("Failed to create allocator!");
        allocator.report_memory_leaks(log::Level::Info);

        let command_buffers = Self::create_commandbuffers(&logical_device, &pools, swapchain.image_count)?;

        
        Ok(Self {
            entry,
            instance,
            is_framebuffer_resized: false,
            debug,
            surface,
            physical_device,
            physical_device_properties,
            physical_device_features,
            queue_families,
            queues,
            device: logical_device,
            swapchain,
            renderpass,
            pipeline,
            pools,
            command_buffers,
            allocator: std::mem::ManuallyDrop::new(allocator),
            game_objects: vec![]
        })
    }

    pub fn create_instance(entry: &ash::Entry, layer_names: &[&str], window: &VulkanWindow) -> Result<ash::Instance, vk::Result> {
        let app_name = std::ffi::CString::new("Reverie Engine").unwrap();
        let engine_name = std::ffi::CString::new("Reverie").unwrap();

        let app_info = vk::ApplicationInfo::builder()
            .application_name(&app_name)
            .engine_name(&engine_name)
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::API_VERSION_1_3);

        let layer_names: Vec<std::ffi::CString> = layer_names
            .iter()
            .map(|&ln| std::ffi::CString::new(ln).unwrap())
            .collect();
        
        let layer_name_pointers: Vec<*const i8> = layer_names
            .iter()
            .map(|layer_name| layer_name.as_ptr())
            .collect();

        let mut extension_name_pointers: Vec<*const i8> = 
            vec![
                ash::extensions::ext::DebugUtils::name().as_ptr(),
            ];
        let required_surface_extensions = ash_window::enumerate_required_extensions(&window.window)
            .unwrap()
            .iter()
            .map(|ext| *ext)
            .collect::<Vec<*const i8>>();
        extension_name_pointers.extend(required_surface_extensions.iter());

        println!("Extensions in use: ");
        for ext in extension_name_pointers.iter() {
            println!("\t{}", unsafe { std::ffi::CStr::from_ptr(*ext).to_str().unwrap() });
        }

        let create_flags = vk::InstanceCreateFlags::default();

        let create_info = vk::InstanceCreateInfo::builder()
            .application_info(&app_info)
            .enabled_layer_names(&layer_name_pointers)
            .enabled_extension_names(&extension_name_pointers)
            .flags(create_flags);

        unsafe { entry.create_instance(&create_info, None) }
    }

    pub fn recreate_swapchain(&mut self) {
        unsafe {
            self.device 
                .device_wait_idle()
                .expect("Failed to wait device idle (recreate swapchain)!")
        };

        unsafe {
            self.device.free_command_buffers(self.pools.graphics_command_pool, &self.command_buffers);
            self.pools.cleanup(&self.device);
            self.pipeline.cleanup(&self.device);
            RenderPass::cleanup(&self.device, self.renderpass);
            self.swapchain.cleanup(&self.device);
        }

        self.swapchain = VulkanSwapchain::new(&self.instance, self.physical_device, &self.device, &self.surface, &self.queue_families)
            .expect("Failed to recreate swapchain.");

        self.renderpass = RenderPass::init(&self.device, self.swapchain.surface_format.format)
            .expect("Failed to recreate renderpass.");

        self.swapchain.create_framebuffers(&self.device, self.renderpass)
            .expect("Failed to recreate framebuffers.");

        self.pipeline = Pipeline::new(&self.device, &self.swapchain, &self.renderpass)
            .expect("Failed to recreate pipeline.");

        self.pools = Pools::new(&self.device, &self.queue_families)
            .expect("Failed to recreate pipeline.");

        self.command_buffers = Self::create_commandbuffers(&self.device, &self.pools, self.swapchain.image_count)
            .expect("Failed to recreate command_buffers.");

        Self::fill_commandbuffers(&self.command_buffers, &self.device, &self.renderpass, &self.swapchain, &self.pipeline, &self.game_objects)
            .expect("Failed to fill commmandbuffers");
    }

    pub fn create_commandbuffers(logical_device: &ash::Device, pools: &Pools, amount: usize) -> Result<Vec<vk::CommandBuffer>, vk::Result> {
        let commandbuffer_allocate_info = vk::CommandBufferAllocateInfo::builder()
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_pool(pools.graphics_command_pool)
            .command_buffer_count(amount as u32);
            
        
        unsafe { logical_device.allocate_command_buffers(&commandbuffer_allocate_info) }
    }

    pub fn fill_commandbuffers(command_buffers: &[vk::CommandBuffer], logical_device: &ash::Device, renderpass: &vk::RenderPass, swapchain: &VulkanSwapchain, pipeline: &Pipeline, game_objects: &Vec<GameObject>
    ) -> Result<(), vk::Result> {
        unsafe {
            logical_device
                .wait_for_fences(&[swapchain.may_begin_drawing[swapchain.current_image]], true, std::u64::MAX)
                .expect("Fence wait failed!");
        }

        for (i, &command_buffer) in command_buffers.iter().enumerate() {
            let commandbuffer_begininfo = vk::CommandBufferBeginInfo::builder();
            unsafe { logical_device.begin_command_buffer(command_buffer, &commandbuffer_begininfo)?; }
        

            let clear_values = [vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.0, 0.0, 0.0, 1.0]
                }}, 
                vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0
                }
            }];

            let renderpass_begininfo = vk::RenderPassBeginInfo::builder()
                .render_pass(*renderpass)
                .framebuffer(swapchain.framebuffers[i])
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x:0, y:0 },
                    extent: swapchain.extent
                })
                .clear_values(&clear_values);

            unsafe {
                logical_device.cmd_begin_render_pass(command_buffer, &renderpass_begininfo, vk::SubpassContents::INLINE);

                let viewports = [vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: swapchain.extent.width as f32,
                    height: swapchain.extent.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }];

                let scissors = [vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: swapchain.extent
                }];
                
                logical_device.cmd_set_viewport(command_buffer, 0, &viewports);
                logical_device.cmd_set_scissor(command_buffer, 0, &scissors);

                for (_i, game_object) in game_objects.iter().enumerate() {
                    logical_device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::GRAPHICS, pipeline.pipeline);
                    match &game_object.mesh.index_buffer {
                        Some(index_buffer) => {
                            logical_device.cmd_bind_index_buffer(command_buffer, index_buffer.get_buffer(), 0, vk::IndexType::UINT32);
                            for vertex_buffer in &game_object.mesh.vertex_buffers {
                                logical_device.cmd_bind_vertex_buffers(command_buffer, 0, &[vertex_buffer.get_buffer()], &[0]);

                                    let push = PushConstantData {
                                        _transform: game_object.transform2d.mat2(),
                                        _offset: game_object.transform2d.translation,
                                        _color: align::Align16(game_object.color)
                                    };
                                    let bytes = push.as_bytes();

                                    logical_device.cmd_push_constants(command_buffer, pipeline.layout, vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT, 0, &bytes);
                                    logical_device.cmd_draw_indexed(command_buffer, index_buffer.get_index_count(), 1, 0, 0, 0);
                                
                            }
                        },
                        None => {
                            for vertex_buffer in &game_object.mesh.vertex_buffers {
                                logical_device.cmd_bind_vertex_buffers(command_buffer, 0, &[vertex_buffer.get_buffer()], &[0]);
                                logical_device.cmd_draw(command_buffer, vertex_buffer.get_vertex_count(), 1, 0, 0);
                            }
                        }
                    }
                }

                logical_device.cmd_end_render_pass(command_buffer);
                logical_device.end_command_buffer(command_buffer)?;
            }
        }
        Ok(())
    }

    pub fn draw_frame(&mut self) {
        self.swapchain.current_image = {self.swapchain.current_image + 1} % self.swapchain.image_count as usize;

        let (image_index, _is_sub_optimal) = unsafe {
            let result = self.swapchain.swapchain_loader.acquire_next_image(
                self.swapchain.swapchain, std::u64::MAX, self.swapchain.image_available[self.swapchain.current_image], vk::Fence::null());

            match result {
                Ok(image_index) => image_index,
                Err(vk_result) => match vk_result {
                    vk::Result::ERROR_OUT_OF_DATE_KHR => {
                        self.recreate_swapchain();
                        return;
                    }
                    _ => panic!("Failed to acquire swapchain image!")
                }
            }
        };

        unsafe {
            self.device.wait_for_fences(&[self.swapchain.may_begin_drawing[self.swapchain.current_image]], true, std::u64::MAX)
                .expect("Fence wait failed!");
        }

        let semaphores_available = [self.swapchain.image_available[self.swapchain.current_image]];
        let waiting_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let semaphores_finished = [self.swapchain.rendering_finished[self.swapchain.current_image]];
        let command_buffers = [self.command_buffers[image_index as usize]];
        let submit_info = [vk::SubmitInfo::builder()
            .wait_semaphores(&semaphores_available)
            .wait_dst_stage_mask(&waiting_stages)
            .command_buffers(&command_buffers)
            .signal_semaphores(&semaphores_finished)
            .build()    
        ];

        unsafe {
            self.device.reset_fences(&[self.swapchain.may_begin_drawing[self.swapchain.current_image]])
                .expect("Fence reset failed!");

            self.device.queue_submit(self.queues.graphics_queue, &submit_info, self.swapchain.may_begin_drawing[self.swapchain.current_image])
                .expect("Failed to submit command buffer!");
        }

        let swapchains = [self.swapchain.swapchain];
        let indices = [image_index];
        let present_info = vk::PresentInfoKHR::builder()
            .wait_semaphores(&semaphores_finished)
            .swapchains(&swapchains)
            .image_indices(&indices);
        
        let result = unsafe { self.swapchain.swapchain_loader.queue_present(self.queues.graphics_queue, &present_info) };

        let is_resized = match result {
            Ok(_) => self.is_framebuffer_resized,
            Err(vk_result) => match vk_result {
                vk::Result::ERROR_OUT_OF_DATE_KHR | vk::Result::SUBOPTIMAL_KHR => true,
                _ => panic!("Failed to present swapchain image")
            }
        };

        if is_resized {
            self.is_framebuffer_resized = false;
            self.recreate_swapchain();
        }
    }
}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().expect("Failed to wait for device idle!");

            for game_object in &mut self.game_objects {
                game_object.mesh.destroy(&self.device, &mut self.allocator);
            }

            self.device.free_command_buffers(self.pools.graphics_command_pool, &self.command_buffers);

            self.pools.cleanup(&self.device);
            self.pipeline.cleanup(&self.device);
            self.device.destroy_render_pass(self.renderpass, None);
            self.swapchain.cleanup(&self.device);
            std::mem::ManuallyDrop::drop(&mut self.allocator);
            self.device.destroy_device(None);
            self.surface.cleanup();
            self.debug.cleanup();
            self.instance.destroy_instance(None)
        };
    }
}

#[repr(C)]
pub struct PushConstantData {
    _transform: uv::Mat2,
    _offset: uv::Vec2,
    _color: align::Align16<uv::Vec3>
}

impl PushConstantData {
    pub unsafe fn as_bytes(&self) -> &[u8] {
        any_as_u8_slice(self)
    }
}