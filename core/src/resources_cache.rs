//! Pinned style-cache inventory snapshots. Kept apart from the cache lookup
//! algorithm so resource accounting cannot affect its keys or output pixels.

use std::mem::size_of_val;
use std::sync::Arc;

use crate::resources::Inventory;
use crate::style::Effect;
use crate::style_render::{PlaneColor, RenderedEffects};

#[derive(Default)]
pub(crate) struct CacheSnapshot {
    pub(crate) planes: Vec<Arc<RenderedEffects>>,
    pub(crate) inherited_effects: Vec<Arc<[Effect]>>,
}

impl CacheSnapshot {
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        for planes in &self.planes {
            inventory.object(&**planes, "style_cache_structure", true);
            inventory.vec(&planes.shape.coverage, "style_cache_planes", true);
            for contributions in [&planes.below, &planes.interior] {
                inventory.vec(contributions, "style_cache_structure", true);
                for contribution in contributions {
                    inventory.vec(&contribution.coverage, "style_cache_planes", true);
                    if let PlaneColor::Gradient(sampler) = &contribution.color {
                        inventory.vec(&sampler.fill.stops, "style_cache_structure", true);
                        // A Shape Burst stroke's field, shared by its
                        // contributions and so recorded once by address.
                        if let Some(field) = sampler.burst() {
                            inventory.record(
                                field.t.as_ptr() as usize,
                                size_of_val(&*field.t),
                                "style_cache_planes",
                                true,
                            );
                        }
                    }
                    // A pattern overlay's PYRAMID level, held by its plane.
                    if let PlaneColor::Pattern(sampler) = &contribution.color {
                        let level = &sampler.level.premultiplied;
                        inventory.record(
                            level.as_ptr() as usize,
                            size_of_val(&**level),
                            "style_cache_planes",
                            true,
                        );
                    }
                }
            }
        }
        for effects in &self.inherited_effects {
            inventory.record(
                effects.as_ptr() as usize,
                size_of_val(&**effects),
                "style_cache_structure",
                true,
            );
            inventory.effects(effects, true);
        }
    }
}
