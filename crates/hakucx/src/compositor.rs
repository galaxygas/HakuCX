/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Draws webview textures into the window framebuffer as premultiplied-alpha quads.
//! Layer order is decided by the caller: wallpaper/clear, page views, then the UI shell.

use std::num::NonZeroU32;

use glow::HasContext;

/// A rectangle in window pixels with a top-left origin (same as CSS / winit).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PixelRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl PixelRect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

pub struct Compositor {
    program: glow::Program,
    vao: glow::VertexArray,
    u_rect: glow::UniformLocation,
    u_viewport: glow::UniformLocation,
    u_color: glow::UniformLocation,
    u_use_texture: glow::UniformLocation,
}

const VERTEX_ES: &str = r#"#version 300 es
precision highp float;
uniform vec4 u_rect;      // x, y, w, h in GL pixels (bottom-left origin)
uniform vec2 u_viewport;  // framebuffer size
out vec2 v_uv;
void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1));
    v_uv = corner;
    vec2 pos = u_rect.xy + corner * u_rect.zw;
    gl_Position = vec4(pos / u_viewport * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const FRAGMENT_ES: &str = r#"#version 300 es
precision mediump float;
in vec2 v_uv;
uniform sampler2D u_texture;
uniform vec4 u_color;
uniform int u_use_texture;
out vec4 frag;
void main() {
    frag = u_use_texture == 1 ? texture(u_texture, v_uv) : u_color;
}
"#;

impl Compositor {
    /// # Safety
    /// The GL context that `gl` wraps must be current.
    pub unsafe fn new(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            let (vs_src, fs_src) = if gl.version().is_embedded {
                (VERTEX_ES.to_owned(), FRAGMENT_ES.to_owned())
            } else {
                (
                    VERTEX_ES.replace("#version 300 es", "#version 330 core"),
                    FRAGMENT_ES.replace("#version 300 es", "#version 330 core"),
                )
            };
            let program = gl.create_program()?;
            let mut shaders = Vec::new();
            for (kind, src) in [
                (glow::VERTEX_SHADER, vs_src),
                (glow::FRAGMENT_SHADER, fs_src),
            ] {
                let shader = gl.create_shader(kind)?;
                gl.shader_source(shader, &src);
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    return Err(gl.get_shader_info_log(shader));
                }
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(gl.get_program_info_log(program));
            }
            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }
            let vao = gl.create_vertex_array()?;
            let uniform = |name: &str| {
                gl.get_uniform_location(program, name)
                    .ok_or_else(|| format!("missing uniform {name}"))
            };
            Ok(Self {
                u_rect: uniform("u_rect")?,
                u_viewport: uniform("u_viewport")?,
                u_color: uniform("u_color")?,
                u_use_texture: uniform("u_use_texture")?,
                program,
                vao,
            })
        }
    }

    /// Start a frame on the currently bound (window) framebuffer.
    ///
    /// # Safety
    /// The window GL context must be current with its framebuffer bound.
    pub unsafe fn begin(&self, gl: &glow::Context, width: u32, height: u32, clear: [f32; 4]) {
        unsafe {
            gl.viewport(0, 0, width as i32, height as i32);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::DEPTH_TEST);
            gl.clear_color(clear[0], clear[1], clear[2], clear[3]);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.uniform_2_f32(Some(&self.u_viewport), width as f32, height as f32);
        }
    }

    fn set_rect(&self, gl: &glow::Context, rect: PixelRect, viewport_h: f32) {
        // Convert top-left origin into GL's bottom-left origin.
        let gl_y = viewport_h - (rect.y + rect.h);
        unsafe { gl.uniform_4_f32(Some(&self.u_rect), rect.x, gl_y, rect.w, rect.h) };
    }

    /// # Safety
    /// Must be called between [`Self::begin`] and the window present.
    pub unsafe fn fill(
        &self,
        gl: &glow::Context,
        rect: PixelRect,
        viewport_h: f32,
        rgba: [f32; 4],
    ) {
        unsafe {
            self.set_rect(gl, rect, viewport_h);
            // Premultiply the solid color.
            let a = rgba[3];
            gl.uniform_4_f32(
                Some(&self.u_color),
                rgba[0] * a,
                rgba[1] * a,
                rgba[2] * a,
                a,
            );
            gl.uniform_1_i32(Some(&self.u_use_texture), 0);
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
        }
    }

    /// # Safety
    /// Must be called between [`Self::begin`] and the window present; `texture` must be a
    /// live texture on the current context.
    pub unsafe fn draw_texture(
        &self,
        gl: &glow::Context,
        texture: u32,
        rect: PixelRect,
        viewport_h: f32,
    ) {
        let Some(id) = NonZeroU32::new(texture) else {
            return;
        };
        unsafe {
            self.set_rect(gl, rect, viewport_h);
            gl.uniform_1_i32(Some(&self.u_use_texture), 1);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(glow::NativeTexture(id)));
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }

    /// # Safety
    /// Restores state Servo's renderer expects after we are done drawing.
    pub unsafe fn end(&self, gl: &glow::Context) {
        unsafe {
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::BLEND);
        }
    }
}
