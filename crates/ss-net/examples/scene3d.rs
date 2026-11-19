//! Build 3D scenes for a few places and print what they contain:
//! `cargo run --release -p ss-net --example scene3d`.
use ss_net::Net;

#[tokio::main]
async fn main() {
    let net = Net::new("https://api.librewxr.net").unwrap();
    let stations = net.radar_stations().await.unwrap_or_default();
    let maps = net.weather_maps().await.ok();
    let frames = maps.map(|m| m.radar_past).unwrap_or_default();
    for (name, lat, lon) in [
        ("Antioch, CA", 38.0, -121.81),
        ("Compiègne, FR", 49.42, 2.83),
        ("Denver, CO", 39.74, -104.99),
    ] {
        let t = std::time::Instant::now();
        match net
            .scene3d(lat, lon, &stations, "https://api.librewxr.net", &frames)
            .await
        {
            Ok(s) => {
                let (lo, hi) = s
                    .terrain
                    .as_ref()
                    .map(|t| {
                        t.heights_m
                            .iter()
                            .fold((f32::MAX, f32::MIN), |(a, b), h| (a.min(*h), b.max(*h)))
                    })
                    .unwrap_or_default();
                let top = s.voxels.iter().map(|v| v.z).fold(0.0, f32::max);
                println!(
                    "{name}: {:?} · {} voxels (top {top:.1} km) · terrain {lo:.0}–{hi:.0} m · {:.1}s",
                    s.source,
                    s.voxels.len(),
                    t.elapsed().as_secs_f32()
                );
            }
            Err(e) => println!("{name}: error {e}"),
        }
    }
}
