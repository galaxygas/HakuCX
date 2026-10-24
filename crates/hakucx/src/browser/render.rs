/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Frame composition: page panes first (each over a white underlay), then the UI shell.

use log::warn;
use servo::RenderingContext;

use super::Browser;
use super::tabs::Pane;
use crate::compositor::PixelRect;

impl Browser {
    pub fn paint(&self) {
        log::trace!("paint");
        if let Err(error) = self.window_context.make_current() {
            warn!("make_current failed: {error:?}");
            return;
        }
        let layout = self.layout.borrow().clone();
        let panes: Vec<(Pane, PixelRect)> = [
            (Pane::Main, Some(layout.page)),
            (Pane::Split, layout.split),
            (Pane::Panel, layout.panel),
        ]
        .into_iter()
        .filter_map(|(pane, rect)| Some((pane, rect?)))
        .filter(|(pane, _)| self.pane_webview(*pane).is_some())
        .collect();

        // Let Servo render each visible view into its texture.
        for (pane, _) in &panes {
            if let Some(webview) = self.pane_webview(*pane) {
                webview.paint();
            }
        }
        if let Some(ui) = self.ui_webview() {
            ui.paint();
        }

        let gl = self.window_context.glow_gl_api();
        let size = self.window.inner_size();
        let viewport_h = size.height as f32;
        let background = self.clear_color();
        self.window_context.prepare_for_rendering();
        unsafe {
            self.compositor
                .begin(&gl, size.width, size.height, background);
            for (pane, rect) in &panes {
                self.compositor
                    .fill(&gl, *rect, viewport_h, [1.0, 1.0, 1.0, 1.0]);
                let texture = self.pane_contexts[pane.index()].texture_id();
                self.compositor
                    .draw_texture(&gl, texture, *rect, viewport_h);
            }
            let full = PixelRect {
                x: 0.0,
                y: 0.0,
                w: size.width as f32,
                h: size.height as f32,
            };
            self.compositor
                .draw_texture(&gl, self.ui_context.texture_id(), full, viewport_h);
            self.compositor.end(&gl);
        }
        self.window_context.present();
    }

    /// The theme background, shown for the instant before the UI shell's first frame.
    fn clear_color(&self) -> [f32; 4] {
        let settings = self.settings.borrow();
        let palette = haku_core::themes::palette_for_settings(
            &settings.appearance,
            &self.user_themes.borrow().themes,
            self.system_dark.get(),
        );
        palette
            .vars
            .get("--hx-frame")
            .and_then(|hex| haku_core::themes::Rgb::parse(hex))
            .map(|c| {
                [
                    c.0 as f32 / 255.0,
                    c.1 as f32 / 255.0,
                    c.2 as f32 / 255.0,
                    1.0,
                ]
            })
            .unwrap_or([0.02, 0.02, 0.03, 1.0])
    }
}
