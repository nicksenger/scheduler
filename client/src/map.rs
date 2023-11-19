use std::collections::HashMap;

use iced::widget::canvas;
use iced::widget::canvas::{Path, Text};
use iced::Color;
use iced::Size;
use iced::{Element, Length, Point, Renderer, Theme};
use schema::{Destination, DestinationName, StatusUpdate};

use super::Message;

// TODO: these should come from BE
const TOTAL_CARRIERS: usize = 10;
const CARRIER_SPEED_MPS: u64 = 30;

pub fn view<'a>(
    destinations: &HashMap<DestinationName, Destination>,
    update: &StatusUpdate,
    // Optimistic client representation of "scheduler-time"
    perceived_time_millis: u64,
) -> Element<'a, Message> {
    let (max_x, max_y) = destinations
        .values()
        .fold((0.0f32, 0.0f32), |(x, y), dest| {
            (x.max(dest.east_m as f32), y.max(dest.north_m as f32))
        });
    let (min_x, min_y) = destinations
        .values()
        .fold((0.0f32, 0.0f32), |(x, y), dest| {
            (x.min(dest.east_m as f32), y.min(dest.north_m as f32))
        });

    let (scale_x, scale_y) = (max_x - min_x, max_y - min_y);
    let origin = ((0.0 - min_x) / scale_x, (0.0 - min_y) / scale_y);

    let dest_positions = destinations
        .values()
        .map(|dest| {
            let y = ((dest.north_m as f32 * -1.0) - min_y) / scale_y;
            let x = (dest.east_m as f32 - min_x) / scale_x;

            (dest.name.to_string(), x, y)
        })
        .collect::<Vec<_>>();

    let carrier_positions = update
        .flights
        .iter()
        .map(|flight| {
            let (east_m, north_m, n) = flight.current_position(
                destinations,
                perceived_time_millis / 1000,
                CARRIER_SPEED_MPS,
            );

            let y = ((north_m * -1.0) - min_y) / scale_y;
            let x = (east_m - min_x) / scale_x;

            (n, x, y)
        })
        .collect::<Vec<_>>();

    canvas(MapCanvas {
        dest_positions,
        carrier_positions,
        origin,
        cache: Default::default(),
    })
    .width(Length::Fixed(600.0))
    .height(Length::Fixed(600.0))
    .into()
}

struct MapCanvas {
    dest_positions: Vec<(String, f32, f32)>,
    carrier_positions: Vec<(usize, f32, f32)>,
    origin: (f32, f32),
    cache: canvas::Cache,
}

impl<'a, Message> canvas::Program<Message, Renderer> for MapCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let (width, height) = (550.0, 550.0);
        let graph = self.cache.draw(renderer, size, |frame| {
            let position = Point::new(width * self.origin.0, height * self.origin.1 + 50.0);
            frame.fill_text(Text {
                content: format!(
                    "Origin ({} carriers available)",
                    TOTAL_CARRIERS - self.carrier_positions.len()
                ),
                position,
                ..Default::default()
            });

            for (name, x, y) in &self.dest_positions {
                let position = Point::new(width * x, height * y + 50.0);
                let dot = Path::circle(position, 5.0);
                frame.fill(&dot, Color::BLACK);
                frame.fill_text(Text {
                    content: name.to_string(),
                    position,
                    ..Default::default()
                });
            }

            for (n, x, y) in &self.carrier_positions {
                let position = Point::new(width * x, height * y + 50.0);
                let symbol = Path::rectangle(position, Size::new(10.0, 10.0));
                frame.fill(&symbol, Color::from_rgb8(0, 0, 255));
                frame.fill_text(Text {
                    content: n.to_string(),
                    position: Point::new(position.x, position.y + 15.0),
                    color: Color::from_rgb8(0, 0, 255),
                    ..Default::default()
                });
            }
        });

        vec![graph]
    }
}
