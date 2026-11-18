//! Application state, background events and top-level layout.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use chrono::NaiveDateTime;
use crossbeam_channel::Receiver;
use egui::{Align, CornerRadius, Layout, Margin, RichText, Ui};
use ss_core::geo::haversine_km;
use ss_core::theme::Palette;
use ss_core::weather::wmo::accent_for;
use ss_core::{
    Alert, Config, Location, LocationsFile, SavedLocation, UnitPref, Weather, WeatherMaps,
};
use ss_map::TileCache;
use ss_net::Net;
use ss_store::Store;

use crate::ai::AiTask;
use crate::prefs::Prefs;
use crate::radar::Radar;
use crate::search::{self, DayRef, Intent, Parsed, Topic};
use crate::searchbar::SearchBar;
use crate::summary;
use crate::theme::{self, c32};
use crate::worker::{Event, Worker};

/// Height of the top header.
const HEADER_H: f32 = 80.0;
/// Width of the places sidebar.
const SIDEBAR_W: f32 = 270.0;
const MAPS_EVERY: Duration = Duration::from_secs(5 * 60);
/// Treat a picked place this close to a saved one as the same place.
const SAME_PLACE_KM: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Forecast,
    Radar,
    Alerts,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AiState {
    Idle,
    /// Waiting for the forecast before asking.
    Waiting,
    Loading(u64),
    Done(String),
    Failed(String),
}

/// A question the user asked in the search bar.
#[derive(Debug, Clone)]
pub struct Answer {
    pub question: String,
    pub location_id: String,
    pub topic: Topic,
    pub day: Option<DayRef>,
    pub ai: AiState,
}

/// Something the UI asked for; applied after drawing.
#[derive(Debug, Clone)]
pub enum Action {
    Select(String),
    Remove(String),
    AddPlace(Location),
    Intent(Intent),
    /// Free text submitted from the search bar.
    Query(String),
    AiSummary,
    DismissAnswer,
    FocusDay(Option<usize>),
    Tab(Tab),
    OpenSettings,
    OpenAssistant,
}

pub struct App {
    ctx: egui::Context,
    worker: Worker,
    rx: Receiver<Event>,
    store: Store,
    pub config: Config,
    pub locations: LocationsFile,
    pub prefs: Prefs,
    pub weather: HashMap<String, Weather>,
    pub alerts: HashMap<String, Vec<Alert>>,
    pub errors: HashMap<String, String>,
    pub loading: HashSet<String>,
    pub maps: Option<WeatherMaps>,
    maps_at: Option<Instant>,
    pub tab: Tab,
    /// Scroll the forecast view to this section on the next frame.
    pub scroll_to: Option<search::View>,
    pub focus_day: Option<usize>,
    search: SearchBar,
    /// A submitted query waiting for its place to be geocoded.
    pending_query: Option<(String, Parsed)>,
    pub answer: Option<Answer>,
    /// AI summary per location id.
    pub ai_summary: HashMap<String, AiState>,
    ai_seq: u64,
    /// What each in-flight AI request is for: `Some(loc)` = summary, `None` = answer.
    ai_targets: HashMap<u64, Option<String>>,
    radar: Radar,
    pub settings_open: bool,
    /// The assistant chat window.
    pub chat: ss_ai::chat::Chat,
    ai_http: reqwest::Client,
    rt: tokio::runtime::Handle,
    pub palette: Palette,
    last_system_dark: Option<bool>,
    last_refresh: Option<Instant>,
    pub locating: bool,
    pub notice: Option<(String, Instant)>,
    /// Locally generated alerts from Settings → Developer (never persisted).
    pub demo_alerts: Vec<Alert>,
    /// The menu bar, revealed with Alt.
    pub menu: ss_assets::menubar::AltMenuBar,
    pub about_open: bool,
    /// The native window title last sent, to avoid resending every frame.
    last_title: String,
    /// When each cached forecast was fetched (unix seconds), by location id.
    pub fetched_at: HashMap<String, i64>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rt: tokio::runtime::Handle,
        store: Store,
    ) -> anyhow::Result<App> {
        let ctx = cc.egui_ctx.clone();
        theme::install_fonts(&ctx);
        let mut config = store.load_config();
        let locations = store.load_locations();
        let mut prefs = Prefs::load(&store);
        if prefs.migrate_into(&mut config.ai) {
            if let Err(e) = store.save_config(&config) {
                tracing::warn!("saving migrated AI settings: {e}");
            }
            prefs.save(&store);
        }
        let net = Net::new(&config.libre_wxr_host)?;
        let (tx, rx) = crossbeam_channel::unbounded();
        let worker = Worker::new(rt.clone(), net, tx, ctx.clone());
        let http = reqwest::Client::builder()
            .user_agent(ss_net::USER_AGENT)
            .build()?;
        let tiles = TileCache::new(
            http,
            rt.clone(),
            Some(store.dir().join("tiles")),
            ctx.clone(),
        );
        let palette = theme::simple_palette(&config, [0x8a, 0x64, 0xff], true);
        theme::apply_palette(&ctx, &palette);

        let mut app = App {
            ctx,
            worker,
            rx,
            store,
            config,
            locations,
            prefs,
            weather: HashMap::new(),
            alerts: HashMap::new(),
            errors: HashMap::new(),
            loading: HashSet::new(),
            maps: None,
            maps_at: None,
            tab: Tab::Forecast,
            scroll_to: None,
            focus_day: None,
            search: SearchBar::default(),
            pending_query: None,
            answer: None,
            ai_summary: HashMap::new(),
            ai_seq: 0,
            ai_targets: HashMap::new(),
            radar: Radar::new(tiles),
            settings_open: false,
            chat: Default::default(),
            ai_http: ss_ai::http_client(),
            rt: rt.clone(),
            palette,
            last_system_dark: None,
            last_refresh: None,
            locating: false,
            notice: None,
            demo_alerts: Vec::new(),
            menu: Default::default(),
            about_open: false,
            last_title: String::new(),
            fetched_at: HashMap::new(),
        };
        // Start from the last good forecasts so the app is useful offline.
        let cache = app.store.load_forecast_cache();
        for (id, (w, at)) in cache.entries {
            if app.locations.saved_locations.iter().any(|l| l.id == id) {
                app.weather.insert(id.clone(), w);
                app.fetched_at.insert(id, at);
            }
        }
        if app.locations.saved_locations.is_empty() {
            app.locating = true;
            app.worker.ip_locate();
        } else if app.active().is_none() {
            app.locations.active_location_id =
                app.locations.saved_locations.first().map(|l| l.id.clone());
        }
        app.refresh_all();
        Ok(app)
    }

    // ----- accessors ------------------------------------------------------

    pub fn active(&self) -> Option<&SavedLocation> {
        let id = self.locations.active_location_id.as_deref()?;
        self.locations.saved_locations.iter().find(|l| l.id == id)
    }

    pub fn active_id(&self) -> Option<String> {
        self.active().map(|l| l.id.clone())
    }

    pub fn active_weather(&self) -> Option<&Weather> {
        self.weather
            .get(self.locations.active_location_id.as_deref()?)
    }

    /// Alerts for the active place, with any demo alerts first.
    pub fn active_alerts(&self) -> Vec<Alert> {
        let real = self
            .locations
            .active_location_id
            .as_deref()
            .and_then(|id| self.alerts.get(id))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        self.demo_alerts.iter().chain(real).cloned().collect()
    }

    /// "Now" in the forecast's own time zone (hourly times are local, no offset).
    pub fn now_for(w: &Weather) -> NaiveDateTime {
        match w.timezone.parse::<chrono_tz::Tz>() {
            Ok(tz) => chrono::Utc::now().with_timezone(&tz).naive_local(),
            Err(_) => chrono::Local::now().naive_local(),
        }
    }

    // ----- persistence & refresh ---------------------------------------

    fn save_locations(&self) {
        if let Err(e) = self.store.save_locations(&self.locations) {
            tracing::warn!("saving locations: {e}");
        }
    }

    pub fn save_config(&self) {
        if let Err(e) = self.store.save_config(&self.config) {
            tracing::warn!("saving config: {e}");
        }
    }

    pub fn save_prefs(&self) {
        self.prefs.save(&self.store);
    }

    fn save_forecast_cache(&self) {
        let cache = ss_store::ForecastCache {
            entries: self
                .weather
                .iter()
                .filter_map(|(id, w)| Some((id.clone(), (w.clone(), *self.fetched_at.get(id)?))))
                .collect(),
        };
        if let Err(e) = self.store.save_forecast_cache(&cache) {
            tracing::warn!("saving forecast cache: {e}");
        }
    }

    /// Quit the app (File → Quit, Ctrl+Q).
    pub fn quit(&self) {
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Take focus out of the search field (a dialog opened).
    pub fn blur_search(&mut self) {
        self.search.blur();
    }

    /// Put focus in the search field on the next frame (Ctrl+F).
    pub fn focus_search(&mut self) {
        self.search.focus_next = true;
    }

    pub fn store_dir(&self) -> String {
        self.store.dir().display().to_string()
    }

    fn refresh_location(&mut self, id: &str) {
        let Some(l) = self.locations.saved_locations.iter().find(|l| l.id == id) else {
            return;
        };
        self.loading.insert(l.id.clone());
        self.worker.forecast(l.id.clone(), l.lat, l.lon);
        self.worker.alerts(l.id.clone(), l.lat, l.lon);
    }

    pub fn refresh_all(&mut self) {
        let ids: Vec<String> = self
            .locations
            .saved_locations
            .iter()
            .map(|l| l.id.clone())
            .collect();
        for id in ids {
            self.refresh_location(&id);
        }
        self.worker.weather_maps();
        self.maps_at = Some(Instant::now());
        self.last_refresh = Some(Instant::now());
    }

    fn tick_timers(&mut self) {
        // 0 = auto-refresh off (radar frames still update).
        let every = match self.config.auto_refresh_minutes {
            0 => Duration::from_secs(u64::MAX / 4),
            m => Duration::from_secs(u64::from(m.max(5)) * 60),
        };
        if self.last_refresh.is_none_or(|t| t.elapsed() >= every) {
            self.refresh_all();
        } else if self.maps_at.is_none_or(|t| t.elapsed() >= MAPS_EVERY) {
            self.worker.weather_maps();
            self.maps_at = Some(Instant::now());
        }
        let next = every.saturating_sub(self.last_refresh.map(|t| t.elapsed()).unwrap_or_default());
        self.ctx.request_repaint_after(next.min(MAPS_EVERY));
    }

    fn update_palette(&mut self) {
        let system_dark = self
            .ctx
            .input(|i| i.raw.system_theme)
            .map(|t| t == egui::Theme::Dark)
            .unwrap_or(true);
        if self.last_system_dark != Some(system_dark) {
            self.last_system_dark = Some(system_dark);
            self.ctx
                .send_viewport_cmd(egui::ViewportCommand::Icon(Some(crate::app_icon(
                    system_dark,
                ))));
        }
        let accent = self
            .active_weather()
            .map(|w| {
                accent_for(
                    w.current.weather_code,
                    w.current.is_day,
                    w.current.temperature_c,
                    w.current.wind_speed_kmh,
                )
            })
            .unwrap_or([0x8a, 0x64, 0xff]);
        let p = theme::simple_palette(&self.config, accent, system_dark);
        if p != self.palette {
            self.palette = p;
            theme::apply_palette(&self.ctx, &p);
        }
    }

    // ----- places ---------------------------------------------------------

    /// Select a place, adding it to the saved list unless one is already there.
    pub fn add_place(&mut self, loc: Location) -> String {
        if let Some(l) = self
            .locations
            .saved_locations
            .iter()
            .find(|l| haversine_km(l.lat, l.lon, loc.lat, loc.lon) < SAME_PLACE_KM)
        {
            let id = l.id.clone();
            self.select(&id);
            return id;
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.locations.saved_locations.push(SavedLocation {
            id: id.clone(),
            label: loc.label,
            lat: loc.lat,
            lon: loc.lon,
            accu_weather_location_key: None,
        });
        self.select(&id);
        self.refresh_location(&id);
        id
    }

    pub fn select(&mut self, id: &str) {
        if self.locations.active_location_id.as_deref() != Some(id) {
            self.locations.active_location_id = Some(id.to_string());
            self.focus_day = None;
            self.save_locations();
        }
    }

    fn remove(&mut self, id: &str) {
        self.locations.saved_locations.retain(|l| l.id != id);
        self.weather.remove(id);
        self.alerts.remove(id);
        self.ai_summary.remove(id);
        if self.locations.active_location_id.as_deref() == Some(id) {
            self.locations.active_location_id =
                self.locations.saved_locations.first().map(|l| l.id.clone());
        }
        self.save_locations();
    }

    // ----- AI ---------------------------------------------------------------

    fn digest_for(&self, id: &str) -> Option<String> {
        let w = self.weather.get(id)?;
        let l = self.locations.saved_locations.iter().find(|l| l.id == id)?;
        let alerts = self.alerts.get(id).map(Vec::as_slice).unwrap_or(&[]);
        Some(ss_ai::digest::forecast_digest(
            w,
            &l.label,
            alerts,
            self.config.units,
            self.config.time_format,
            Self::now_for(w),
        ))
    }

    /// AI is on and fully configured.
    pub fn ai_ready(&self) -> bool {
        self.config.ai.endpoint().is_some()
    }

    /// Context for the assistant: the active place first, then the others.
    fn assistant_context(&self) -> String {
        let mut ids: Vec<String> = self.active_id().into_iter().collect();
        ids.extend(
            self.locations
                .saved_locations
                .iter()
                .map(|l| l.id.clone())
                .filter(|id| Some(id) != self.locations.active_location_id.as_ref()),
        );
        let digests: Vec<String> = ids.iter().filter_map(|id| self.digest_for(id)).collect();
        if digests.is_empty() {
            "No forecast is loaded yet.\n".to_string()
        } else {
            digests.join("\n")
        }
    }

    fn start_ai(&mut self, task: AiTask, target: Option<String>, loc_id: &str) -> Option<u64> {
        let endpoint = self.config.ai.endpoint()?;
        let digest = self.digest_for(loc_id)?;
        self.ai_seq += 1;
        let seq = self.ai_seq;
        self.ai_targets.insert(seq, target);
        self.worker.ai(seq, endpoint, task, digest);
        Some(seq)
    }

    pub fn request_ai_summary(&mut self) {
        let Some(id) = self.active_id() else { return };
        if !self.ai_ready() {
            self.settings_open = true;
            return;
        }
        let state = match self.start_ai(AiTask::Summarize, Some(id.clone()), &id) {
            Some(seq) => AiState::Loading(seq),
            None => AiState::Waiting,
        };
        self.ai_summary.insert(id, state);
    }

    fn ask_ai_for_answer(&mut self) {
        let Some(a) = self.answer.clone() else { return };
        if !self.ai_ready() || !matches!(a.ai, AiState::Waiting) {
            return;
        }
        if let Some(seq) = self.start_ai(AiTask::Ask(a.question.clone()), None, &a.location_id) {
            if let Some(ans) = &mut self.answer {
                ans.ai = AiState::Loading(seq);
            }
        }
    }

    // ----- queries ------------------------------------------------------------

    fn submit_query(&mut self, text: String) {
        let parsed = search::parse(&text);
        if let Some(place) = parsed.place.clone() {
            // An exact-ish saved place wins without a network round trip.
            let best = search::rank(
                &place,
                self.locations
                    .saved_locations
                    .iter()
                    .map(|l| l.label.as_str()),
            )
            .into_iter()
            .next();
            let saved = best.and_then(|(i, s)| {
                let label = &self.locations.saved_locations[i].label;
                let head = label.split(',').next().unwrap_or(label).trim();
                (s >= 60 && head.eq_ignore_ascii_case(place.trim()))
                    .then(|| self.locations.saved_locations[i].id.clone())
            });
            match saved {
                Some(id) => {
                    self.select(&id);
                    self.finish_query(text, parsed);
                }
                None => {
                    self.worker.geocode(place.clone());
                    self.pending_query = Some((text, parsed));
                    self.notice = Some((format!("Looking up “{place}”…"), Instant::now()));
                }
            }
        } else {
            self.finish_query(text, parsed);
        }
    }

    /// The query's place (if any) is now active; apply day / question parts.
    fn finish_query(&mut self, text: String, parsed: Parsed) {
        self.tab = Tab::Forecast;
        if let Some(day) = parsed.day {
            self.focus_day = self
                .active_weather()
                .and_then(|w| summary::day_index(&w.daily, day))
                .or(match day {
                    DayRef::Today => Some(0),
                    DayRef::Tomorrow => Some(1),
                    DayRef::Weekday(_) => None,
                });
            if self.focus_day.is_some_and(|i| i > 0) && parsed.topic.is_none() {
                self.scroll_to = Some(search::View::Week);
            }
        }
        let topic = parsed
            .topic
            .or((parsed.is_question).then_some(Topic::General));
        if let (Some(topic), Some(id)) = (topic, self.active_id()) {
            self.answer = Some(Answer {
                question: text,
                location_id: id,
                topic,
                day: parsed.day,
                ai: if self.ai_ready() {
                    AiState::Waiting
                } else {
                    AiState::Idle
                },
            });
            self.scroll_to = Some(search::View::Forecast);
            self.ask_ai_for_answer();
        }
    }

    fn apply_intent(&mut self, intent: Intent) {
        match intent {
            Intent::View(v) => match v {
                search::View::Radar => self.tab = Tab::Radar,
                search::View::Alerts => self.tab = Tab::Alerts,
                v => {
                    self.tab = Tab::Forecast;
                    self.scroll_to = Some(v);
                }
            },
            Intent::Refresh => self.refresh_all(),
            Intent::Settings => self.settings_open = true,
            Intent::Assistant => self.chat.show_window(),
            Intent::UseMyLocation => {
                self.locating = true;
                self.worker.ip_locate();
            }
            Intent::Units(u) => {
                self.config.units = match u {
                    search::Units::Celsius => UnitPref::C,
                    search::Units::Fahrenheit => UnitPref::F,
                };
                self.save_config();
            }
            Intent::Summarize => {
                self.tab = Tab::Forecast;
                self.scroll_to = Some(search::View::Forecast);
                if self.ai_ready() {
                    self.request_ai_summary();
                }
            }
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Select(id) => self.select(&id),
            Action::Remove(id) => self.remove(&id),
            Action::AddPlace(loc) => {
                self.add_place(loc);
                self.tab = Tab::Forecast;
            }
            Action::Intent(i) => self.apply_intent(i),
            Action::Query(q) => self.submit_query(q),
            Action::AiSummary => self.request_ai_summary(),
            Action::DismissAnswer => self.answer = None,
            Action::FocusDay(d) => self.focus_day = d,
            Action::Tab(t) => {
                self.tab = t;
                self.search.blur();
            }
            Action::OpenSettings => {
                self.settings_open = true;
                self.search.blur();
            }
            Action::OpenAssistant => {
                self.chat.show_window();
                self.search.blur();
            }
        }
    }

    // ----- events ---------------------------------------------------------------

    fn drain_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Weather {
                    location_id,
                    result,
                } => {
                    self.loading.remove(&location_id);
                    match result {
                        Ok(w) => {
                            self.weather.insert(location_id.clone(), w);
                            self.fetched_at
                                .insert(location_id.clone(), chrono::Utc::now().timestamp());
                            self.save_forecast_cache();
                            self.errors.remove(&location_id);
                            let is_active =
                                self.locations.active_location_id.as_deref() == Some(&location_id);
                            if is_active
                                && self.prefs.ai_auto_summary
                                && self.ai_ready()
                                && !matches!(
                                    self.ai_summary.get(&location_id),
                                    Some(AiState::Loading(_))
                                )
                            {
                                self.request_ai_summary();
                            }
                            if matches!(self.ai_summary.get(&location_id), Some(AiState::Waiting)) {
                                self.request_ai_summary();
                            }
                            if self
                                .answer
                                .as_ref()
                                .is_some_and(|a| a.location_id == location_id)
                            {
                                // A weekday focus can only resolve once the days are known.
                                self.ask_ai_for_answer();
                            }
                        }
                        Err(e) => {
                            tracing::warn!("forecast for {location_id}: {e}");
                            self.errors.insert(location_id, friendly_error(&e));
                        }
                    }
                }
                Event::Alerts {
                    location_id,
                    result,
                } => match result {
                    Ok(a) => {
                        self.alerts.insert(location_id, a);
                    }
                    Err(e) => tracing::warn!("alerts for {location_id}: {e}"),
                },
                Event::Geocoded { query, results } => {
                    let pending = self
                        .pending_query
                        .as_ref()
                        .is_some_and(|(_, p)| p.place.as_deref() == Some(query.as_str()));
                    if pending {
                        let (text, parsed) = self.pending_query.take().expect("checked");
                        match results.into_iter().next() {
                            Some(loc) => {
                                self.add_place(loc);
                                self.notice = None;
                                self.finish_query(text, parsed);
                            }
                            None => {
                                self.notice =
                                    Some((format!("Couldn't find “{query}”."), Instant::now()));
                            }
                        }
                    } else {
                        self.search.set_results(query, results);
                    }
                }
                Event::IpLocated(loc) => {
                    self.locating = false;
                    match loc {
                        Some(l) => {
                            let id = self.add_place(l.clone());
                            if l.label.trim().is_empty() {
                                self.worker.reverse_geocode(id, l.lat, l.lon);
                            }
                        }
                        None => {
                            self.notice = Some((
                                "Couldn't detect your location — search for a place instead."
                                    .into(),
                                Instant::now(),
                            ));
                        }
                    }
                }
                Event::ReverseGeocoded { location_id, label } => {
                    if let Some(l) = self
                        .locations
                        .saved_locations
                        .iter_mut()
                        .find(|l| l.id == location_id)
                    {
                        l.label = label;
                        self.save_locations();
                    }
                }
                Event::Maps(r) => match r {
                    Ok(m) => self.maps = Some(m),
                    Err(e) => tracing::warn!("weather maps: {e}"),
                },
                Event::Ai { seq, result } => {
                    let Some(target) = self.ai_targets.remove(&seq) else {
                        continue;
                    };
                    let state = match result {
                        Ok(t) => AiState::Done(t),
                        Err(e) => AiState::Failed(e),
                    };
                    match target {
                        Some(loc) => {
                            if self.ai_summary.get(&loc) == Some(&AiState::Loading(seq)) {
                                self.ai_summary.insert(loc, state);
                            }
                        }
                        None => {
                            if let Some(a) = &mut self.answer {
                                if a.ai == AiState::Loading(seq) {
                                    a.ai = state;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ----- UI -------------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        let s = theme::scheme(&p);
        let tf = self.config.time_format;
        let clock_fmt = match tf {
            ss_core::TimeFormat::H12 => "%-I:%M %p",
            ss_core::TimeFormat::H24 => "%H:%M",
        };
        let now = match self.active_weather() {
            Some(w) => App::now_for(w),
            None => chrono::Local::now().naive_local(),
        };
        ui.horizontal_centered(|ui| {
            // Brand + last refresh, as wide as the sidebar below it.
            ui.allocate_ui_with_layout(
                egui::vec2(SIDEBAR_W - 20.0, 58.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let logo = theme::logomark(ui, &p, 36.0);
                    ui.add(logo);
                    let stamp = self
                        .last_refresh
                        .map(|t| {
                            // Same zone as the clock.
                            let at =
                                now - chrono::Duration::from_std(t.elapsed()).unwrap_or_default();
                            format!("Updated {}", at.format(clock_fmt))
                        })
                        .unwrap_or_else(|| "Not updated yet".into());
                    ui.label(RichText::new(stamp).size(12.5).color(s.on_surface_variant));
                },
            );
            ui.add_space(4.0);

            // The search bar fills the space up to the right-hand controls.
            let right_w = 360.0;
            let search_w = (ui.available_width() - right_w - 8.0).clamp(240.0, 900.0);
            let places = &self.locations.saved_locations;
            if let Some(a) = self.search.show(ui, &p, search_w, places, self.ai_ready()) {
                actions.push(a);
            }
            if let Some(q) = self.search.wants_geocode() {
                self.worker.geocode(q);
            }

            // Right-hand controls on one shared centre line (the search
            // bar's), placed explicitly so they align exactly.
            let row = ui.available_rect_before_wrap();
            let cy = row.center().y;
            ui.allocate_rect(row, egui::Sense::hover());
            let h = 40.0;
            let mut right = row.right();
            let mut take = |w: f32| {
                let r = egui::Rect::from_min_size(
                    egui::pos2(right - w, cy - h / 2.0),
                    egui::vec2(w, h),
                );
                right -= w + 8.0;
                r
            };
            if crate::views::round_button_at(ui, &s, take(h), "⚙", "Settings (Ctrl+,)").clicked()
            {
                actions.push(Action::OpenSettings);
            }
            let items: Vec<(String, bool, Option<egui::Color32>)> = [UnitPref::F, UnitPref::C]
                .iter()
                .map(|u| {
                    let t = if *u == UnitPref::F { "°F" } else { "°C" };
                    (t.to_string(), self.config.units == *u, None)
                })
                .collect();
            let gw = crate::views::connected_group_width(ui, &items, 54.0);
            if let Some(i) = crate::views::connected_group_in(ui, &s, &items, take(gw), 54.0) {
                let u = if i == 0 { UnitPref::F } else { UnitPref::C };
                if u != self.config.units {
                    actions.push(Action::Intent(Intent::Units(match u {
                        UnitPref::C => search::Units::Celsius,
                        UnitPref::F => search::Units::Fahrenheit,
                    })));
                }
            }
            let refresh = take(h);
            if !self.loading.is_empty() {
                crate::m3::loading_indicator(
                    ui.painter(),
                    refresh.center(),
                    40.0,
                    ui.input(|i| i.time),
                    s.primary,
                    Some(s.primary_container),
                );
            } else if crate::views::round_button_at(ui, &s, refresh, "↻", "Refresh (F5)").clicked()
            {
                actions.push(Action::Intent(Intent::Refresh));
            }
            ui.painter().text(
                egui::pos2(right, cy),
                egui::Align2::RIGHT_CENTER,
                format!("{}\n{}", now.format("%a %-d %b"), now.format(clock_fmt)),
                egui::FontId::proportional(12.5),
                s.on_surface_variant,
            );
        });
    }

    /// Place title and coordinates, with the view tabs on the right.
    fn title_row(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        let s = theme::scheme(&p);
        let n = self.active_alerts().len();
        ui.horizontal(|ui| {
            if let Some(l) = self.active() {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(
                        RichText::new(l.label.split(',').next().unwrap_or(&l.label).trim())
                            .font(theme::display_font(40.0))
                            .color(s.on_surface),
                    );
                    ui.label(
                        RichText::new(ss_core::Location::new(l.lat, l.lon, "").coord_label())
                            .size(13.0)
                            .color(s.on_surface_variant),
                    );
                });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let tabs = [Tab::Forecast, Tab::Radar, Tab::Alerts];
                let items: Vec<(String, bool, Option<egui::Color32>)> = tabs
                    .iter()
                    .map(|t| {
                        let text = match t {
                            Tab::Forecast => "Forecast".to_string(),
                            Tab::Radar => "Radar".to_string(),
                            Tab::Alerts if n > 0 => format!("Alerts · {n}"),
                            Tab::Alerts => "Alerts".to_string(),
                        };
                        let badge = (*t == Tab::Alerts && n > 0).then_some(s.error);
                        (text, self.tab == *t, badge)
                    })
                    .collect();
                ui.allocate_ui_with_layout(
                    egui::vec2(340.0, 44.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        if let Some(i) = crate::views::connected_group(ui, &s, &items, 44.0, 104.0)
                        {
                            actions.push(Action::Tab(tabs[i]));
                        }
                    },
                );
            });
        });
        ui.add_space(12.0);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.drain_events();
        self.tick_timers();
        self.update_palette();
        let p = self.palette;
        let mut actions: Vec<Action> = Vec::new();

        crate::menu::shortcuts(ui.ctx(), self, &mut actions);
        // The native title bar names the place, like a document title.
        let title = match self.active() {
            Some(l) => format!("{} — Silver Skies", l.label),
            None => "Silver Skies".to_string(),
        };
        if title != self.last_title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
        if self.menu.update(ui.ctx(), self.config.menu_bar_always) {
            crate::menu::bar(ui, self, &mut actions);
        }

        let header = egui::Panel::top("top")
            .exact_size(HEADER_H)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(c32(p.bg))
                    .inner_margin(Margin::symmetric(20, 10)),
            )
            .show(ui, |ui| self.top_bar(ui, &mut actions));
        let header_rect = header.response.rect;

        // The shared top glow along the header's top edge (below an open menu bar): the sky accent, pulsing
        // in the alert's colour while a severe alert is active.
        {
            if self.config.top_glow_enabled {
                let alerts = self.active_alerts();
                let pulse = ss_core::alerts::colors::active_pulse_color(&alerts);
                let params = ss_assets::glow::GlowParams {
                    color: pulse.unwrap_or(p.a0),
                    pulsing: pulse.is_some(),
                    dark: theme::is_dark(&p),
                };
                let t = ui.input(|i| i.time);
                let painter = ss_assets::glow::glow_painter(ui.ctx());
                ss_assets::glow::draw_top_glow(&painter, header_rect, &params, t);
                ui.ctx().request_repaint_after(Duration::from_millis(33));
            }
        }

        egui::Panel::left("places")
            .exact_size(SIDEBAR_W)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(c32(p.bg)).inner_margin(Margin {
                left: 20,
                right: 10,
                top: 16,
                bottom: 16,
            }))
            .show(ui, |ui| crate::views::places(ui, self, &mut actions));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(c32(p.bg)).inner_margin(Margin {
                left: 10,
                right: 20,
                top: 14,
                bottom: 0,
            }))
            .show(ui, |ui| {
                self.title_row(ui, &mut actions);
                match self.tab {
                    Tab::Forecast => crate::views::forecast(ui, self, &mut actions),
                    Tab::Alerts => crate::views::alerts(ui, self, &mut actions),
                    Tab::Radar => {
                        let place = self.active().map(|l| (l.lat, l.lon, l.label.clone()));
                        let alerts = self.active_alerts();
                        let tz = self.active_weather().map(|w| w.timezone.clone());
                        let frame = egui::Frame::new()
                            .corner_radius(CornerRadius::same(theme::RADIUS))
                            .inner_margin(Margin::ZERO);
                        frame.show(ui, |ui| {
                            ui.set_min_size(ui.available_size() - egui::vec2(0.0, 18.0));
                            self.radar.show(
                                ui,
                                &p,
                                self.maps.as_ref(),
                                place.as_ref().map(|(a, b, c)| (*a, *b, c.as_str())),
                                &alerts,
                                tz.as_deref(),
                            );
                        });
                    }
                }
            });

        // Transient notice, bottom centre.
        if let Some((msg, at)) = &self.notice {
            if at.elapsed() < Duration::from_secs(6) || self.pending_query.is_some() {
                let msg = msg.clone();
                egui::Area::new(egui::Id::unique("notice"))
                    .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
                    .show(ui.ctx(), |ui| {
                        // A Material snackbar: inverse surface, rounded.
                        let s = theme::scheme(&p);
                        egui::Frame::new()
                            .fill(s.inverse_surface)
                            .corner_radius(crate::m3::corner::SMALL)
                            .inner_margin(Margin::symmetric(18, 14))
                            .show(ui, |ui| {
                                ui.label(RichText::new(msg).color(s.inverse_on_surface));
                            });
                    });
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            } else {
                self.notice = None;
            }
        }

        if self.settings_open {
            crate::settings::window(ui.ctx(), self);
        }
        if self.about_open {
            crate::menu::about(ui.ctx(), self);
        }
        if self.chat.open || self.chat.is_busy() {
            let theme = crate::theme::chat_theme(&p);
            let label = self
                .active()
                .map(|l| l.label.clone())
                .unwrap_or_else(|| "your places".into());
            let system =
                ss_ai::digest::assistant_system_prompt("Silver Skies", &self.assistant_context());
            let settings = self.config.ai.clone();
            let (rt, client) = (self.rt.clone(), self.ai_http.clone());
            if self.chat.show(
                ui.ctx(),
                "Silver Skies",
                &theme,
                &settings,
                &label,
                move || system,
                &rt,
                &client,
            ) == ss_ai::chat::ChatAction::OpenSettings
            {
                self.settings_open = true;
            }
        }

        for a in actions {
            self.apply(a);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_locations();
    }
}

/// A short, human message for a network error string from `ss-net`.
pub fn friendly_error(e: &str) -> String {
    if e.contains("HTTP 429") {
        "The forecast service is busy right now. Silver Skies will try again shortly.".into()
    } else if let Some(code) = e
        .strip_prefix("HTTP ")
        .and_then(|r| r.split_whitespace().next())
    {
        format!("The forecast service returned an error ({code}). Try refreshing in a minute.")
    } else if e.starts_with("network error") {
        "Couldn't reach the forecast service. Check your connection.".into()
    } else {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::friendly_error;

    #[test]
    fn errors_are_humanized() {
        assert!(friendly_error("HTTP 429 (rate limited) from x").contains("busy"));
        assert!(friendly_error("HTTP 500 (server error) from x").contains("(500)"));
        assert!(friendly_error("network error: x timed out").contains("connection"));
        assert_eq!(friendly_error("parse error: bad"), "parse error: bad");
    }
}
