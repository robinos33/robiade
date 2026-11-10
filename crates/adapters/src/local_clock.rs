//! Heure locale de la machine, celle que launchd suit pour `StartCalendarInterval`.

use robiade_core::routine_schedule::{Date, LocalTime};

/// L'instant `millis`, à la minute, dans le fuseau de la machine.
pub fn local_time(millis: i64) -> LocalTime {
    let secs = millis.div_euclid(1000) as libc::time_t;
    // SAFETY : `localtime_r` n'écrit que dans `tm`, une valeur locale initialisée à zéro.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    LocalTime {
        date: Date { year: tm.tm_year + 1900, month: (tm.tm_mon + 1) as u8, day: tm.tm_mday as u8 },
        hour: tm.tm_hour as u8,
        minute: tm.tm_min as u8,
    }
}

/// L'instant de cette heure locale ; `mktime` tranche les heures ambiguës ou sautées du
/// changement d'heure. `None` si le système ne sait pas la représenter.
pub fn millis_of(t: LocalTime) -> Option<i64> {
    // SAFETY : `tm` est une valeur locale entièrement initialisée ; `mktime` la normalise en place.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = t.date.year - 1900;
    tm.tm_mon = i32::from(t.date.month) - 1;
    tm.tm_mday = i32::from(t.date.day);
    tm.tm_hour = i32::from(t.hour);
    tm.tm_min = i32::from(t.minute);
    tm.tm_isdst = -1;
    let secs = unsafe { libc::mktime(&mut tm) };
    (secs != -1).then(|| secs as i64 * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minute_round_trips_through_the_local_clock() {
        let now = 1_794_297_687_591;
        let t = local_time(now);
        assert_eq!(millis_of(t), Some(now - now.rem_euclid(60_000)));
    }
}
