/**
 * Schedules in plain words, and when a schedule fires next. Cron is read in
 * this computer's local time zone, which is the zone the scheduler on the
 * same computer evaluates it in. Shared by Routines (the next hour) and
 * Calendar (the runs in each day's margin), so both read a schedule alike.
 */

const DAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

export function scheduleWords(schedule: Record<string, unknown>): string {
  const minutes = schedule.every_minutes;
  if (typeof minutes === "number" && minutes > 0) {
    if (minutes % 1440 === 0) return `Every ${minutes / 1440} ${minutes === 1440 ? "day" : "days"}`;
    if (minutes % 60 === 0) return `Every ${minutes / 60} ${minutes === 60 ? "hour" : "hours"}`;
    return `Every ${minutes} ${minutes === 1 ? "minute" : "minutes"}`;
  }
  if (typeof schedule.cron === "string") {
    const [minute, hour, day, month, weekday] = schedule.cron.split(" ");
    const everyMinutes = /^\*\/(\d+)$/.exec(minute ?? "");
    if (everyMinutes && hour === "*" && day === "*" && month === "*" && weekday === "*") return `Every ${everyMinutes[1]} minutes`;
    if (minute && hour && day && month && weekday && /^\d+$/.test(minute)) {
      const time = /^\d+$/.test(hour) ? ` at ${hour.padStart(2, "0")}:${minute.padStart(2, "0")}` : "";
      if (day === "*" && month === "*" && weekday === "*" && time) return `Every day${time}`;
      if (day === "1" && month === "*" && weekday === "*" && time) return `Every month on day 1${time}`;
      if (/^\d+$/.test(day) && /^\d+(,\d+)*$/.test(month) && weekday === "*" && time) {
        const names = month.split(",").map(value => MONTHS[Number(value) - 1]);
        if (names.every(Boolean)) return `On day ${day} of ${names.length === 1 ? names[0] : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`}${time}`;
      }
      if (day === "*" && month === "*" && /^[0-6]$/.test(weekday) && time) return `Every ${DAYS[Number(weekday)]}${time}`;
      if (day === "*" && month === "*" && weekday === "1-5" && time) return `Every weekday${time}`;
      if (hour === "*" && day === "*" && month === "*" && weekday === "*") return `Every hour at ${minute} minutes past`;
      const interval = /^\*\/(\d+)$/.exec(hour);
      if (interval && day === "*" && month === "*" && weekday === "*") return `Every ${interval[1]} hours at ${minute} minutes past`;
    }
    const parts = schedule.cron.split(" ");
    if (parts.length === 5) {
      const units = ["minute", "hour", "day of the month", "month", "day of the week"];
      const describe = (part: string, unit: string): string => {
        if (part === "*") return `every ${unit}`;
        return part.split(",").map(piece => {
          const step = /^(\*|\d+-\d+)\/(\d+)$/.exec(piece);
          if (step) return step[1] === "*" ? `every ${step[2]} ${unit}s` : `every ${step[2]} ${unit}s from ${step[1]?.replace("-", " to ")}`;
          if (/^\d+-\d+$/.test(piece)) return `${unit}s ${piece.replace("-", " through ")}`;
          return /^\d+$/.test(piece) ? `${unit} ${piece}` : `${unit} managed in Orbit`;
        }).join(" or ");
      };
      return `Scheduled for ${parts.map((part, index) => describe(part, units[index] ?? "time")).join(", ")}`;
    }
    return "On a schedule managed in Orbit";
  }
  if (schedule.deliveries_landed && typeof schedule.deliveries_landed === "object") {
    const delivery = schedule.deliveries_landed as Record<string, unknown>;
    const count = delivery.threshold;
    const wait = delivery.max_wait_minutes;
    if (typeof count === "number" && typeof wait === "number") return `After ${count} ${count === 1 ? "delivery" : "deliveries"} or up to ${wait} minutes`;
    return "After deliveries are completed";
  }
  return "On a schedule managed in Orbit";
}

/** True when the schedule waits for an event rather than the clock. */
export function eventDriven(schedule: Record<string, unknown>): boolean {
  return typeof schedule.cron !== "string" && typeof schedule.every_minutes !== "number";
}

/** The values one cron field allows, or null when the field is not understood. */
function cronField(field: string, min: number, max: number): Set<number> | null {
  const values = new Set<number>();
  for (const piece of field.split(",")) {
    const match = /^(\*|(\d+)(?:-(\d+))?)(?:\/(\d+))?$/.exec(piece);
    if (match === null) return null;
    const step = match[4] === undefined ? 1 : Number(match[4]);
    let from = min;
    let to = max;
    if (match[1] !== "*") {
      from = Number(match[2]);
      to = match[3] === undefined ? (match[4] === undefined ? from : max) : Number(match[3]);
    }
    if (step < 1 || from < min || to > max || from > to) return null;
    for (let value = from; value <= to; value += step) values.add(value);
  }
  return values;
}

interface Cron {
  minutes: Set<number>;
  hours: Set<number>;
  days: Set<number>;
  months: Set<number>;
  weekdays: Set<number>;
  dayRestricted: boolean;
  weekdayRestricted: boolean;
}

function parseCron(cron: string): Cron | null {
  const parts = cron.trim().split(/\s+/);
  if (parts.length !== 5) return null;
  const [minute = "", hour = "", day = "", month = "", weekday = ""] = parts;
  const minutes = cronField(minute, 0, 59);
  const hours = cronField(hour, 0, 23);
  const days = cronField(day, 1, 31);
  const months = cronField(month, 1, 12);
  const weekdays = cronField(weekday, 0, 7);
  if (!minutes || !hours || !days || !months || !weekdays) return null;
  if (weekdays.delete(7)) weekdays.add(0);
  return { minutes, hours, days, months, weekdays, dayRestricted: day !== "*", weekdayRestricted: weekday !== "*" };
}

function dayMatches(cron: Cron, date: Date): boolean {
  if (!cron.months.has(date.getMonth() + 1)) return false;
  const day = cron.days.has(date.getDate());
  const weekday = cron.weekdays.has(date.getDay());
  // Standard cron: when both day fields are restricted, either one matches.
  if (cron.dayRestricted && cron.weekdayRestricted) return day || weekday;
  return day && weekday;
}

/** Every fire strictly after `from` and no later than `until`, at most `limit`. */
export function cronFires(cron: string, from: Date, until: Date, limit = 64): Date[] | null {
  const parsed = parseCron(cron);
  if (parsed === null) return null;
  const fires: Date[] = [];
  const day = new Date(from.getFullYear(), from.getMonth(), from.getDate());
  while (day <= until && fires.length < limit) {
    if (dayMatches(parsed, day)) {
      for (const hour of [...parsed.hours].sort((a, b) => a - b)) {
        for (const minute of [...parsed.minutes].sort((a, b) => a - b)) {
          const at = new Date(day.getFullYear(), day.getMonth(), day.getDate(), hour, minute);
          if (at > from && at <= until) fires.push(at);
          if (fires.length >= limit) return fires;
        }
      }
    }
    day.setDate(day.getDate() + 1);
  }
  return fires;
}

/**
 * When a clock schedule fires next after `now`, or null when that cannot be
 * worked out here (an event schedule, an unreadable cron, or an interval
 * that has never fired).
 */
export function nextFire(schedule: Record<string, unknown>, now: Date, lastFiredAt: Date | null): Date | null {
  if (typeof schedule.cron === "string") {
    const year = new Date(now.getTime() + 400 * 86_400_000);
    return cronFires(schedule.cron, now, year, 1)?.[0] ?? null;
  }
  const minutes = schedule.every_minutes;
  if (typeof minutes === "number" && minutes > 0 && lastFiredAt !== null) {
    const step = minutes * 60_000;
    let at = lastFiredAt.getTime() + step;
    if (at <= now.getTime()) at += Math.ceil((now.getTime() - at) / step) * step;
    return new Date(at);
  }
  return null;
}

/** Fires of a clock schedule within the hour after `now`. */
export function firesInNextHour(schedule: Record<string, unknown>, now: Date, lastFiredAt: Date | null): Date[] {
  const until = new Date(now.getTime() + 3_600_000);
  if (typeof schedule.cron === "string") return cronFires(schedule.cron, now, until, 60) ?? [];
  const next = nextFire(schedule, now, lastFiredAt);
  const minutes = schedule.every_minutes;
  if (next === null || typeof minutes !== "number") return [];
  const fires: Date[] = [];
  for (let at = next.getTime(); at <= until.getTime() && fires.length < 60; at += minutes * 60_000) fires.push(new Date(at));
  return fires;
}
