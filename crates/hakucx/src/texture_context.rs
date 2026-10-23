/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! An offscreen [`RenderingContext`] that renders a webview into a GL texture owned by
//! HakuCX, so the compositor can draw it with alpha blending (Servo's own
//! `OffscreenRenderingContext` only exposes an opaque framebuffer blit).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use euclid::default::Size2D as UntypedSize2D;
use servo::{DeviceIntRect, RenderingContext, RgbaImage, WindowRenderingContext};
use surfman::{Connection, Surface, SurfaceTexture};
use winit::dpi::PhysicalSize;

use gleam::gl::{self, Gl};

struct Target {
    gl: Rc<dyn Gl>,
    framebuffer: u32,
    texture: u32,
    depth: u32,
}

impl Target {
    fn new(gl: Rc<dyn Gl>, size: PhysicalSize<u32>) -> Self {
        let framebuffer = gl.gen_framebuffers(1)[0];
        gl.bind_framebuffer(gl::FRAMEBUFFER, framebuffer);

        let texture = gl.gen_textures(1)[0];
        gl.bind_texture(gl::TEXTURE_2D, texture);
        gl.tex_image_2d(
            gl::TEXTURE_2D,
            0,
            gl::RGBA as i32,
            size.width as i32,
            size.height as i32,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            None,
        );
        gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        gl.framebuffer_texture_2d(
            gl::FRAMEBUFFER,
            gl::COLOR_ATTACHMENT0,
            gl::TEXTURE_2D,
            texture,
            0,
        );
        gl.bind_texture(gl::TEXTURE_2D, 0);

        let depth = gl.gen_renderbuffers(1)[0];
        gl.bind_renderbuffer(gl::RENDERBUFFER, depth);
        gl.renderbuffer_storage(
            gl::RENDERBUFFER,
            gl::DEPTH_COMPONENT24,
            size.width as i32,
            size.height as i32,
        );
        gl.framebuffer_renderbuffer(
            gl::FRAMEBUFFER,
            gl::DEPTH_ATTACHMENT,
            gl::RENDERBUFFER,
            depth,
        );

        Self {
            gl,
            framebuffer,
            texture,
            depth,
        }
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        self.gl.bind_framebuffer(gl::FRAMEBUFFER, 0);
        self.gl.delete_textures(&[self.texture]);
        self.gl.delete_renderbuffers(&[self.depth]);
        self.gl.delete_framebuffers(&[self.framebuffer]);
    }
}

pub struct TextureRenderingContext {
    parent: Rc<WindowRenderingContext>,
    size: Cell<PhysicalSize<u32>>,
    target: RefCell<Target>,
}

impl TextureRenderingContext {
    pub fn new(parent: Rc<WindowRenderingContext>, size: PhysicalSize<u32>) -> Self {
        let size = PhysicalSize::new(size.width.max(1), size.height.max(1));
        let target = Target::new(parent.gleam_gl_api(), size);
        Self {
            parent,
            size: Cell::new(size),
            target: RefCell::new(target),
        }
    }

    /// The GL texture holding the most recently painted frame (premultiplied RGBA).
    pub fn texture_id(&self) -> u32 {
        self.target.borrow().texture
    }
}

impl RenderingContext for TextureRenderingContext {
    fn prepare_for_rendering(&self) {
        let target = self.target.borrow();
        target
            .gl
            .bind_framebuffer(gl::FRAMEBUFFER, target.framebuffer);
    }

    fn read_to_image(&self, rect: DeviceIntRect) -> Option<RgbaImage> {
        let target = self.target.borrow();
        let gl = &target.gl;
        gl.bind_framebuffer(gl::FRAMEBUFFER, target.framebuffer);
        gl.bind_vertex_array(0);
        let (w, h) = (rect.width(), rect.height());
        if w <= 0 || h <= 0 {
            return None;
        }
        let pixels = gl.read_pixels(rect.min.x, rect.min.y, w, h, gl::RGBA, gl::UNSIGNED_BYTE);
        // GL rows are bottom-up; images are top-down.
        let stride = w as usize * 4;
        let mut flipped = vec![0u8; pixels.len()];
        for y in 0..h as usize {
            let src = (h as usize - 1 - y) * stride;
            flipped[y * stride..(y + 1) * stride].copy_from_slice(&pixels[src..src + stride]);
        }
        RgbaImage::from_raw(w as u32, h as u32, flipped)
    }

    fn size(&self) -> PhysicalSize<u32> {
        self.size.get()
    }

    fn resize(&self, new_size: PhysicalSize<u32>) {
        let new_size = PhysicalSize::new(new_size.width.max(1), new_size.height.max(1));
        if new_size == self.size.get() {
            return;
        }
        let _ = self.parent.make_current();
        *self.target.borrow_mut() = Target::new(self.parent.gleam_gl_api(), new_size);
        self.size.set(new_size);
    }

    fn present(&self) {}

    fn make_current(&self) -> Result<(), surfman::Error> {
        self.parent.make_current()
    }

    fn gleam_gl_api(&self) -> Rc<dyn Gl> {
        self.parent.gleam_gl_api()
    }

    fn glow_gl_api(&self) -> Arc<glow::Context> {
        self.parent.glow_gl_api()
    }

    fn create_texture(
        &self,
        surface: Surface,
    ) -> Option<(SurfaceTexture, u32, UntypedSize2D<i32>)> {
        self.parent.create_texture(surface)
    }

    fn destroy_texture(&self, surface_texture: SurfaceTexture) -> Option<Surface> {
        self.parent.destroy_texture(surface_texture)
    }

    fn connection(&self) -> Option<Connection> {
        self.parent.connection()
    }

    fn refresh_driver(&self) -> Option<Rc<dyn servo::RefreshDriver>> {
        self.parent.refresh_driver()
    }
}
