//! The 29-day itinerary of the December 2028 trip (from the trip plan),
//! shown in the agenda as all-day events coloured by city. Day N falls on
//! `tripStart + N - 1`, so changing the dates in Ajustes moves them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TripDay {
    pub day: u32,
    pub city: String,
    pub title: String,
    /// Linha Viagem stations practised for this day.
    #[serde(default)]
    pub st: Vec<String>,
}

pub const CITIES: [&str; 6] = ["Tokyo", "Hokkaido", "Kanazawa", "Kyoto", "Kinosaki", "Osaka"];

pub fn city_jp(city: &str) -> &'static str {
    match city {
        "Tokyo" => "東京",
        "Hokkaido" => "北海道",
        "Kanazawa" => "金沢",
        "Kyoto" => "京都",
        "Kinosaki" => "城崎",
        "Osaka" => "大阪",
        _ => "",
    }
}

/// Agenda colour of each city.
pub fn city_color(city: &str) -> &'static str {
    match city {
        "Tokyo" => "#d7262e",
        "Hokkaido" => "#2b7bd1",
        "Kanazawa" => "#b8860b",
        "Kyoto" => "#7d47c4",
        "Kinosaki" => "#0c8d7d",
        "Osaka" => "#e67e00",
        _ => "#d7262e",
    }
}

pub fn itinerary() -> Vec<TripDay> {
    serde_json::from_str(include_str!("trip28_days.json")).expect("trip28_days.json")
}

#[cfg(test)]
mod tests {
    #[test]
    fn has_29_days_in_order() {
        let t = super::itinerary();
        assert_eq!(t.len(), 29);
        assert!(t.iter().enumerate().all(|(i, d)| d.day as usize == i + 1));
        assert!(t.iter().all(|d| super::CITIES.contains(&d.city.as_str())));
    }
}
