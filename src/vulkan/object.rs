use ash::vk;
use gpu_allocator::vulkan::Allocator;

use super::vertex_buffer::VertexBuffer;
use super::index_buffer::IndexBuffer;
use super::vertex::Vertex;

pub struct Object {
    pub vertex_buffers: Vec<VertexBuffer>,
    pub index_buffer: Option<IndexBuffer>
}

impl Object {
    pub fn new(device: &ash::Device, allocator: &mut Allocator, vertex_count: usize, index_count: usize) -> Result<Object, vk::Result> {
        let mut vertex_buffers = vec![];
        let vertex_buffer = VertexBuffer::new(device, allocator, VertexBuffer::get_vertex_buffer_size(vertex_count));
        vertex_buffers.push(vertex_buffer);
        if index_count > 0 {
            let index_buffer = IndexBuffer::new(device, allocator, IndexBuffer::get_index_buffer_size(index_count));
            Ok(Object {
                vertex_buffers,
                index_buffer: Some(index_buffer)
            })
        } else {
            Ok(Object {
                vertex_buffers,
                index_buffer: None
            })
        }
    }

    pub fn update_vertices_buffer(&mut self, data: &[Vertex]) {
        self.vertex_buffers[0].update_buffer(data);
    }

    pub fn update_indices_buffer(&mut self, data: &[u32]) {
        match self.index_buffer {
            Some(ref mut index_buffer) => {
                index_buffer.update_buffer(data);
            },
            None => {
                println!("Tried to update indices buffer on a Object created without an index buffer!");
            }
        }
    }

    pub fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        for vertex_buffer in &mut self.vertex_buffers {
            vertex_buffer.destroy(device, allocator);
        }
        if let Some(index_buffer) = &mut self.index_buffer {
            index_buffer.destroy(device, allocator);
        }
    }

    pub fn get_vertex_buffers(&self) -> Vec<&VertexBuffer> {
        let mut vertex_buffers: Vec<&VertexBuffer> = Vec::new();
        for vertex_buffer in &self.vertex_buffers {
            vertex_buffers.push(vertex_buffer)
        }
        vertex_buffers
    }

    pub fn get_index_buffers(&self) -> Vec<&IndexBuffer> {
        let mut index_buffers: Vec<&IndexBuffer> = Vec::new();
        for index_buffer in &self.index_buffer {
            index_buffers.push(index_buffer)
        }
        index_buffers
    }
}