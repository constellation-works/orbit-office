/**
 * Deterministic stand-ins for the subprocess supervisors.
 * Timers stay armed until the test fires them, and `process.kill` records
 * the requested id instead of signalling a real process.
 */
import cp from "node:child_process";
import { EventEmitter } from "node:events";
import { syncBuiltinESMExports } from "node:module";

export function installSupervisionDoubles() {
  const original = {
    spawn: cp.spawn,
    kill: process.kill,
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout,
  };
  const children = [];
  const timers = [];
  const signals = [];

  cp.spawn = (_command, args, options) => {
    const child = new EventEmitter();
    child.pid = 41000 + children.length;
    child.exitCode = null;
    child.signalCode = null;
    child.closed = false;
    child.args = args;
    child.options = options;
    child.stdout = new EventEmitter();
    child.stderr = new EventEmitter();
    child.stdin = new EventEmitter();
    child.stdin.end = () => {};
    child.finish = (code = 0, signal = null) => {
      child.exitCode = code;
      child.signalCode = signal;
      child.closed = true;
      child.emit("exit", code, signal);
      child.emit("close", code, signal);
    };
    child.kill = (signal) => {
      signals.push(snapshot(child, child.pid, signal));
      return true;
    };
    children.push(child);
    return child;
  };
  syncBuiltinESMExports();

  process.kill = (pid, signal) => {
    const child = children.find((candidate) => candidate.pid === -pid);
    signals.push(snapshot(child, pid, signal));
    return true;
  };

  globalThis.setTimeout = (fn, ms, ...args) => {
    const timer = {
      fn,
      ms,
      args,
      cleared: false,
      unref() {},
      ref() {},
      hasRef() {
        return false;
      },
      refresh() {},
    };
    timers.push(timer);
    return timer;
  };
  globalThis.clearTimeout = (timer) => {
    if (timer !== null && typeof timer === "object" && "fn" in timer && "cleared" in timer) {
      timer.cleared = true;
      return;
    }
    original.clearTimeout(timer);
  };

  return {
    children,
    timers,
    signals,
    restore() {
      cp.spawn = original.spawn;
      syncBuiltinESMExports();
      process.kill = original.kill;
      globalThis.setTimeout = original.setTimeout;
      globalThis.clearTimeout = original.clearTimeout;
    },
  };
}

function snapshot(child, pid, signal) {
  return {
    pid,
    signal,
    closed: child?.closed ?? null,
    exitCode: child?.exitCode ?? null,
    signalCode: child?.signalCode ?? null,
  };
}

/** The single timer registered for `ms`. Throws when the count is not one. */
export function timerAt(timers, ms) {
  const matches = timers.filter((timer) => timer.ms === ms);
  if (matches.length !== 1) {
    throw new Error(`expected one ${ms} ms timer, found ${matches.length}`);
  }
  return matches[0];
}
