//! Silver Skies map engine for egui.
//!
//! Web Mercator viewport, async tile cache, and a stack of layers (raster tiles,
//! WMS images, polygons, markers, wind barbs, range rings, selection) drawn by [`MapWidget`].

pub mod heights;
pub mod layers;
pub mod legend;
pub mod map;
pub mod radar_loop;
pub mod storm_layer;
pub mod tiles;
pub mod viewport;

pub use layers::{
    ancestor, barb_components, basemap, bbox_intersects, carto_basemap, esri_basemap,
    esri_hillshade, esri_imagery, esri_labels, esri_roads, geometry_bbox, lonlat_from_3857,
    lonlat_to_3857, point_in_polygon, triangulate, Barb, Basemap, DrawCtx, GeoImageLayer, Layer,
    LayerStack, LightningLayer, Marker, MarkerKind, MarkerLayer, PolyItem, PolygonLayer,
    RangeRingLayer, RasterTileLayer, SelectionLayer, TriData, WindBarbLayer, WmsImageLayer,
};
pub use legend::{draw_color_legend, draw_color_legend_inline, radar_dbz_stops};
pub use map::{attribution_for, MapResponse, MapWidget, BACKGROUND};
pub use radar_loop::{LoopClock, LoopFrameSource, RadarLoopLayer, TileUrlFn};
pub use storm_layer::{StormLayer, StormMark, StormShape};
pub use tiles::{TileCache, TileKey, TileState, TileStats};
pub use viewport::{FlyAnim, TileId, Units, Viewport};
