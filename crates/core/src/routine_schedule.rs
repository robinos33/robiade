//! Planning d'une routine, tel que launchd le lit dans `StartCalendarInterval` : une liste de
//! créneaux, où une clé absente vaut « n'importe quelle valeur », comme dans crontab(5). Robiade
//! n'écrit que des plannings hebdomadaires (des jours, des heures) ; il lit et décrit les autres.

use std::collections::BTreeSet;

/// Jour de la semaine, numéroté comme launchd : 0 (ou 7) pour dimanche, 1 pour lundi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    pub const ALL: [Weekday; 7] = [
        Self::Monday,
        Self::Tuesday,
        Self::Wednesday,
        Self::Thursday,
        Self::Friday,
        Self::Saturday,
        Self::Sunday,
    ];

    /// Numéro de launchd ; 7 est un autre nom de dimanche.
    pub fn from_launchd(n: u8) -> Option<Self> {
        match n {
            0 | 7 => Some(Self::Sunday),
            1..=6 => Some(Self::ALL[n as usize - 1]),
            _ => None,
        }
    }

    pub fn launchd(self) -> u8 {
        match self {
            Self::Sunday => 0,
            other => other as u8 + 1,
        }
    }

    /// Lundi = 1, comme pour l'interface (ISO 8601).
    pub fn iso(self) -> u8 {
        self as u8 + 1
    }

    pub fn from_iso(n: u8) -> Option<Self> {
        (1..=7).contains(&n).then(|| Self::ALL[n as usize - 1])
    }

    fn short(self) -> &'static str {
        ["lun", "mar", "mer", "jeu", "ven", "sam", "dim"][self as usize]
    }
}

/// Un créneau de `StartCalendarInterval`. `None` : n'importe quelle valeur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CalendarSlot {
    pub month: Option<u8>,
    pub day: Option<u8>,
    pub weekday: Option<Weekday>,
    pub hour: Option<u8>,
    pub minute: Option<u8>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ScheduleError {
    #[error("{0} hors bornes")]
    OutOfRange(&'static str),
    #[error("choisis au moins un jour")]
    NoDay,
    #[error("choisis au moins une heure")]
    NoTime,
    #[error("heure illisible : « {0} » (attendu 9:00 ou 17h30)")]
    BadTime(String),
    #[error("{MAX_SLOTS} créneaux au plus")]
    TooMany,
}

/// Au-delà, le plist devient illisible et la routine n'est probablement plus une routine.
const MAX_SLOTS: usize = 200;

impl CalendarSlot {
    /// Valeurs de launchd, bornées : un mois de 1 à 12, un jour de 1 à 31, etc.
    pub fn new(
        month: Option<i64>,
        day: Option<i64>,
        weekday: Option<i64>,
        hour: Option<i64>,
        minute: Option<i64>,
    ) -> Result<Self, ScheduleError> {
        fn bounded(v: Option<i64>, lo: i64, hi: i64, what: &'static str) -> Result<Option<u8>, ScheduleError> {
            match v {
                None => Ok(None),
                Some(v) if (lo..=hi).contains(&v) => Ok(Some(v as u8)),
                Some(_) => Err(ScheduleError::OutOfRange(what)),
            }
        }
        let weekday = match bounded(weekday, 0, 7, "jour de la semaine")? {
            None => None,
            Some(n) => Weekday::from_launchd(n),
        };
        Ok(Self {
            month: bounded(month, 1, 12, "mois")?,
            day: bounded(day, 1, 31, "jour du mois")?,
            weekday,
            hour: bounded(hour, 0, 23, "heure")?,
            minute: bounded(minute, 0, 59, "minute")?,
        })
    }

    /// Le jour convient. Comme crontab(5) : quand le jour du mois et celui de la semaine sont tous
    /// deux fixés, l'un ou l'autre suffit.
    fn matches_day(&self, date: Date) -> bool {
        if self.month.is_some_and(|m| m != date.month) {
            return false;
        }
        match (self.day, self.weekday) {
            (None, None) => true,
            (Some(d), None) => d == date.day,
            (None, Some(w)) => w == date.weekday(),
            (Some(d), Some(w)) => d == date.day || w == date.weekday(),
        }
    }

    /// Premier instant du créneau dans la journée, au plus tôt à `from` (minutes depuis minuit).
    fn first_minute_from(&self, from: u16) -> Option<u16> {
        let hours: Vec<u16> = self.hour.map_or_else(|| (0..24).collect(), |h| vec![h as u16]);
        let minutes: Vec<u16> = self.minute.map_or_else(|| (0..60).collect(), |m| vec![m as u16]);
        hours.iter().flat_map(|h| minutes.iter().map(move |m| h * 60 + m)).find(|&t| t >= from)
    }
}

/// Planning complet : aucun créneau, la routine ne part jamais sur l'horloge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Schedule(Vec<CalendarSlot>);

impl Schedule {
    pub fn new(slots: Vec<CalendarSlot>) -> Result<Self, ScheduleError> {
        if slots.len() > MAX_SLOTS {
            return Err(ScheduleError::TooMany);
        }
        Ok(Self(slots))
    }

    pub fn slots(&self) -> &[CalendarSlot] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// La forme que Robiade sait éditer : chaque jour retenu à chaque heure retenue, rien d'autre.
    pub fn weekly(&self) -> Option<WeeklySchedule> {
        if self.0.is_empty() || self.0.iter().any(|s| s.month.is_some() || s.day.is_some()) {
            return None;
        }
        let mut times = BTreeSet::new();
        for s in &self.0 {
            times.insert((s.hour?, s.minute?));
        }
        let every_day = self.0.iter().all(|s| s.weekday.is_none());
        let days: BTreeSet<Weekday> = if every_day {
            Weekday::ALL.into_iter().collect()
        } else {
            self.0.iter().map(|s| s.weekday).collect::<Option<_>>()?
        };
        let weekly = WeeklySchedule { days, times };
        // Chaque jour à chaque heure, rien de plus : lundi 9h et mardi 17h ne font pas « lun, mar 9h, 17h ».
        let mut grid = weekly.grid(!every_day);
        let mut theirs = self.0.clone();
        grid.sort();
        theirs.sort();
        theirs.dedup();
        (grid == theirs).then_some(weekly)
    }

    /// Le planning en clair : « lun–ven 9h00 ».
    pub fn describe(&self) -> String {
        if self.0.is_empty() {
            return "jamais sur l'horloge".into();
        }
        if let Some(weekly) = self.weekly() {
            return weekly.describe();
        }
        self.0.iter().map(describe_slot).collect::<Vec<_>>().join(" · ")
    }

    /// Prochain déclenchement strictement après `now`, en heure locale ; `None` s'il n'y en a pas
    /// dans les 400 jours (un 31 février, par exemple).
    pub fn next_after(&self, now: LocalTime) -> Option<LocalTime> {
        let start = now.date.days();
        for offset in 0..400 {
            let date = Date::from_days(start + offset);
            let from = if offset == 0 { now.minute_of_day() + 1 } else { 0 };
            if from >= 24 * 60 {
                continue;
            }
            let first = self
                .0
                .iter()
                .filter(|s| s.matches_day(date))
                .filter_map(|s| s.first_minute_from(from))
                .min();
            if let Some(t) = first {
                return Some(LocalTime { date, hour: (t / 60) as u8, minute: (t % 60) as u8 });
            }
        }
        None
    }
}

/// Des jours de la semaine et des heures : chaque jour, à chaque heure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeeklySchedule {
    days: BTreeSet<Weekday>,
    times: BTreeSet<(u8, u8)>,
}

impl WeeklySchedule {
    pub fn new(days: BTreeSet<Weekday>, times: BTreeSet<(u8, u8)>) -> Result<Self, ScheduleError> {
        if days.is_empty() {
            return Err(ScheduleError::NoDay);
        }
        if times.is_empty() {
            return Err(ScheduleError::NoTime);
        }
        if times.iter().any(|&(h, m)| h > 23 || m > 59) {
            return Err(ScheduleError::OutOfRange("heure"));
        }
        let weekly = Self { days, times };
        if weekly.slot_count() > MAX_SLOTS {
            return Err(ScheduleError::TooMany);
        }
        Ok(weekly)
    }

    /// `9:00`, `9h`, `17h30`, `07:05`.
    pub fn parse_time(raw: &str) -> Result<(u8, u8), ScheduleError> {
        let bad = || ScheduleError::BadTime(raw.to_owned());
        let lower = raw.trim().to_lowercase();
        let (h, m) = lower.split_once([':', 'h']).unwrap_or((&lower, ""));
        let number = |s: &str| -> Result<u8, ScheduleError> {
            if s.is_empty() || s.len() > 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            s.parse().map_err(|_| bad())
        };
        let hour = number(h)?;
        let minute = if m.is_empty() { 0 } else { number(m)? };
        (hour <= 23 && minute <= 59).then_some((hour, minute)).ok_or_else(bad)
    }

    pub fn days(&self) -> &BTreeSet<Weekday> {
        &self.days
    }

    pub fn times(&self) -> &BTreeSet<(u8, u8)> {
        &self.times
    }

    fn every_day(&self) -> bool {
        self.days.len() == 7
    }

    fn slot_count(&self) -> usize {
        if self.every_day() { self.times.len() } else { self.days.len() * self.times.len() }
    }

    /// Tous les jours : un créneau par heure, sans jour ; sinon un par jour et par heure.
    pub fn schedule(&self) -> Schedule {
        Schedule(self.grid(!self.every_day()))
    }

    /// Les créneaux, un par jour retenu (`by_day`) ou sans jour : n'importe lequel.
    fn grid(&self, by_day: bool) -> Vec<CalendarSlot> {
        let at = |weekday, (hour, minute): (u8, u8)| CalendarSlot {
            weekday,
            hour: Some(hour),
            minute: Some(minute),
            ..CalendarSlot::default()
        };
        if by_day {
            self.days.iter().flat_map(|&d| self.times.iter().map(move |&t| at(Some(d), t))).collect()
        } else {
            self.times.iter().map(|&t| at(None, t)).collect()
        }
    }

    pub fn describe(&self) -> String {
        let times: Vec<String> = self.times.iter().map(|&(h, m)| format!("{h}h{m:02}")).collect();
        let times = match times.as_slice() {
            [one] => one.clone(),
            [rest @ .., last] => format!("{} et {last}", rest.join(", ")),
            [] => String::new(),
        };
        format!("{} {times}", describe_days(&self.days))
    }
}

/// « tous les jours », « lun–ven », « lun, mer, ven », « sam, dim ».
fn describe_days(days: &BTreeSet<Weekday>) -> String {
    if days.len() == 7 {
        return "tous les jours".into();
    }
    let mut runs: Vec<(Weekday, Weekday)> = Vec::new();
    for &d in days {
        match runs.last_mut() {
            Some((_, end)) if *end as u8 + 1 == d as u8 => *end = d,
            _ => runs.push((d, d)),
        }
    }
    runs.iter()
        .flat_map(|&(a, b)| match b as u8 - a as u8 {
            0 => vec![a.short().to_owned()],
            1 => vec![a.short().to_owned(), b.short().to_owned()],
            _ => vec![format!("{}–{}", a.short(), b.short())],
        })
        .collect::<Vec<_>>()
        .join(", ")
}

const MONTHS: [&str; 12] = [
    "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre",
    "décembre",
];

fn describe_slot(s: &CalendarSlot) -> String {
    let mut when = Vec::new();
    if let Some(w) = s.weekday {
        when.push(w.short().to_owned());
    }
    match (s.day, s.month) {
        (Some(1), m) => when.push(format!("le 1er{}", m.map_or(String::new(), |m| format!(" {}", MONTHS[m as usize - 1])))),
        (Some(d), m) => when.push(format!("le {d}{}", m.map_or(String::new(), |m| format!(" {}", MONTHS[m as usize - 1])))),
        (None, Some(m)) => when.push(format!("en {}", MONTHS[m as usize - 1])),
        (None, None) => {}
    }
    let time = match (s.hour, s.minute) {
        (Some(h), Some(m)) => format!("{h}h{m:02}"),
        (None, Some(m)) => format!("toutes les heures à :{m:02}"),
        (Some(h), None) => format!("chaque minute de {h}h"),
        (None, None) => "chaque minute".into(),
    };
    if when.is_empty() {
        if s.hour.is_some() && s.minute.is_some() { format!("tous les jours {time}") } else { time }
    } else {
        format!("{} {time}", when.join(" "))
    }
}

/// Date du calendrier, sans fuseau : celle de l'horloge locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Date {
    /// Jours depuis le 1970-01-01 (algorithme « days from civil » de Howard Hinnant).
    fn days(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    fn from_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Self { year, month, day }
    }

    pub fn weekday(self) -> Weekday {
        // Le 1970-01-01 était un jeudi.
        Weekday::ALL[(self.days() + 3).rem_euclid(7) as usize]
    }
}

/// Instant de l'horloge locale, à la minute : c'est la granularité de launchd.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalTime {
    pub date: Date,
    pub hour: u8,
    pub minute: u8,
}

impl LocalTime {
    fn minute_of_day(self) -> u16 {
        u16::from(self.hour) * 60 + u16::from(self.minute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> LocalTime {
        LocalTime { date: Date { year, month, day }, hour, minute }
    }

    fn weekdays_at(hour: u8, minute: u8) -> Schedule {
        let days = (1..=5).map(|n| Weekday::from_iso(n).unwrap()).collect();
        WeeklySchedule::new(days, [(hour, minute)].into()).unwrap().schedule()
    }

    #[test]
    fn launchd_counts_sunday_as_zero_or_seven() {
        assert_eq!(Weekday::from_launchd(0), Some(Weekday::Sunday));
        assert_eq!(Weekday::from_launchd(7), Some(Weekday::Sunday));
        assert_eq!(Weekday::from_launchd(1), Some(Weekday::Monday));
        assert_eq!(Weekday::Monday.launchd(), 1);
        assert_eq!(Weekday::Sunday.launchd(), 0);
        assert_eq!(Weekday::from_launchd(8), None);
    }

    #[test]
    fn calendar_dates_round_trip_and_know_their_weekday() {
        for days in [-1, 0, 59, 365, 11_016, 20_767, 40_000] {
            assert_eq!(Date::from_days(days).days(), days);
        }
        assert_eq!(Date { year: 1970, month: 1, day: 1 }.weekday(), Weekday::Thursday);
        assert_eq!(Date { year: 2026, month: 11, day: 10 }.weekday(), Weekday::Tuesday);
        assert_eq!(Date { year: 2028, month: 2, day: 29 }.weekday(), Weekday::Tuesday);
    }

    #[test]
    fn a_slot_is_bounded_like_launchd() {
        assert!(CalendarSlot::new(None, None, Some(1), Some(9), Some(0)).is_ok());
        assert_eq!(CalendarSlot::new(Some(13), None, None, None, None), Err(ScheduleError::OutOfRange("mois")));
        assert_eq!(CalendarSlot::new(None, None, None, Some(24), None), Err(ScheduleError::OutOfRange("heure")));
        assert_eq!(CalendarSlot::new(None, None, None, None, Some(60)), Err(ScheduleError::OutOfRange("minute")));
    }

    #[test]
    fn a_working_day_schedule_reads_as_such() {
        assert_eq!(weekdays_at(9, 0).describe(), "lun–ven 9h00");
        let every_day = WeeklySchedule::new(Weekday::ALL.into(), [(17, 30), (9, 0)].into()).unwrap();
        assert_eq!(every_day.describe(), "tous les jours 9h00 et 17h30");
        let some = [Weekday::Monday, Weekday::Wednesday, Weekday::Friday].into();
        assert_eq!(WeeklySchedule::new(some, [(10, 0)].into()).unwrap().describe(), "lun, mer, ven 10h00");
        let weekend = [Weekday::Saturday, Weekday::Sunday].into();
        assert_eq!(WeeklySchedule::new(weekend, [(8, 5)].into()).unwrap().describe(), "sam, dim 8h05");
    }

    #[test]
    fn an_every_day_schedule_has_no_weekday() {
        let s = WeeklySchedule::new(Weekday::ALL.into(), [(9, 0)].into()).unwrap().schedule();
        assert_eq!(s.slots(), [CalendarSlot { hour: Some(9), minute: Some(0), ..Default::default() }]);
    }

    #[test]
    fn only_a_days_by_times_grid_is_editable() {
        assert!(weekdays_at(9, 0).weekly().is_some());
        let monday_nine = CalendarSlot { weekday: Some(Weekday::Monday), hour: Some(9), minute: Some(0), ..Default::default() };
        let tuesday_five = CalendarSlot { weekday: Some(Weekday::Tuesday), hour: Some(17), minute: Some(0), ..Default::default() };
        assert!(Schedule::new(vec![monday_nine, tuesday_five]).unwrap().weekly().is_none());
        let hourly = CalendarSlot { minute: Some(15), ..Default::default() };
        assert!(Schedule::new(vec![hourly]).unwrap().weekly().is_none());
        let monthly = CalendarSlot { day: Some(1), hour: Some(3), minute: Some(0), ..Default::default() };
        assert!(Schedule::new(vec![monthly]).unwrap().weekly().is_none());
        assert!(Schedule::default().weekly().is_none());
    }

    #[test]
    fn other_schedules_are_described_slot_by_slot() {
        let hourly = CalendarSlot { minute: Some(15), ..Default::default() };
        assert_eq!(Schedule::new(vec![hourly]).unwrap().describe(), "toutes les heures à :15");
        let monthly = CalendarSlot { day: Some(1), hour: Some(3), minute: Some(0), ..Default::default() };
        assert_eq!(Schedule::new(vec![monthly]).unwrap().describe(), "le 1er 3h00");
        let new_year = CalendarSlot { month: Some(1), day: Some(2), hour: Some(0), minute: Some(5), ..Default::default() };
        assert_eq!(Schedule::new(vec![new_year]).unwrap().describe(), "le 2 janvier 0h05");
        assert_eq!(Schedule::default().describe(), "jamais sur l'horloge");
    }

    #[test]
    fn the_next_run_is_strictly_after_now() {
        let s = weekdays_at(9, 0);
        // Mardi 10 novembre 2026.
        assert_eq!(s.next_after(at(2026, 11, 10, 8, 59)), Some(at(2026, 11, 10, 9, 0)));
        assert_eq!(s.next_after(at(2026, 11, 10, 9, 0)), Some(at(2026, 11, 11, 9, 0)));
        // Vendredi soir : lundi.
        assert_eq!(s.next_after(at(2026, 11, 13, 18, 0)), Some(at(2026, 11, 16, 9, 0)));
        // Fin d'année et de journée.
        assert_eq!(s.next_after(at(2026, 12, 31, 23, 59)), Some(at(2027, 1, 1, 9, 0)));
    }

    #[test]
    fn wildcards_fire_at_the_next_matching_minute() {
        let hourly = Schedule::new(vec![CalendarSlot { minute: Some(15), ..Default::default() }]).unwrap();
        assert_eq!(hourly.next_after(at(2026, 11, 10, 9, 20)), Some(at(2026, 11, 10, 10, 15)));
        let minutely = Schedule::new(vec![CalendarSlot::default()]).unwrap();
        assert_eq!(minutely.next_after(at(2026, 11, 10, 23, 59)), Some(at(2026, 11, 11, 0, 0)));
    }

    #[test]
    fn day_of_month_or_weekday_fires_like_cron() {
        // Le 1er du mois ou un dimanche.
        let either = CalendarSlot { day: Some(1), weekday: Some(Weekday::Sunday), hour: Some(3), minute: Some(0), ..Default::default() };
        let s = Schedule::new(vec![either]).unwrap();
        assert_eq!(s.next_after(at(2026, 11, 10, 12, 0)), Some(at(2026, 11, 15, 3, 0)));
        assert_eq!(s.next_after(at(2026, 11, 29, 12, 0)), Some(at(2026, 12, 1, 3, 0)));
    }

    #[test]
    fn an_impossible_date_never_fires() {
        let feb_31 = CalendarSlot { month: Some(2), day: Some(31), hour: Some(0), minute: Some(0), ..Default::default() };
        assert_eq!(Schedule::new(vec![feb_31]).unwrap().next_after(at(2026, 11, 10, 0, 0)), None);
        assert_eq!(Schedule::default().next_after(at(2026, 11, 10, 0, 0)), None);
    }

    #[test]
    fn times_are_typed_the_french_way_or_with_a_colon() {
        assert_eq!(WeeklySchedule::parse_time("9:00"), Ok((9, 0)));
        assert_eq!(WeeklySchedule::parse_time(" 17h30 "), Ok((17, 30)));
        assert_eq!(WeeklySchedule::parse_time("9h"), Ok((9, 0)));
        assert_eq!(WeeklySchedule::parse_time("07:05"), Ok((7, 5)));
        for bad in ["", "24:00", "9:60", "9:5:0", "neuf", "-1", "9:000"] {
            assert!(WeeklySchedule::parse_time(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_weekly_schedule_needs_a_day_and_a_time() {
        assert_eq!(WeeklySchedule::new(BTreeSet::new(), [(9, 0)].into()), Err(ScheduleError::NoDay));
        assert_eq!(WeeklySchedule::new(Weekday::ALL.into(), BTreeSet::new()), Err(ScheduleError::NoTime));
    }
}
