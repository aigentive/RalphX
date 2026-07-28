/**
 * The relay wiring is where multi-environment isolation is actually enforced: the Rust
 * proxy publishes every environment's frames on one fixed channel, so a bus must see its
 * own environment's frames and nothing else.
 */

import { describe, expect, it, vi } from "vitest";

import { MockEventBus } from "../event-bus";
import { NetworkEventBus } from "./network-event-bus";
import { attachRemoteStreamRelay, type RemoteStreamTarget } from "./stream-relay";
import {
  REMOTE_STREAM_CLOSED_EVENT,
  REMOTE_STREAM_FRAME_EVENT,
  type RemoteServerFrame,
} from "./stream-frames";

const ENV = "env-uuid-1";
const OTHER = "env-uuid-2";

function target(environmentId = ENV) {
  const frames: RemoteServerFrame[] = [];
  const closed = vi.fn();
  const stub: RemoteStreamTarget = {
    environmentId: () => environmentId,
    handleFrame: (frame) => frames.push(frame),
    handleStreamClosed: closed,
  };
  return { stub, frames, closed };
}

const EVENT_FRAME: RemoteServerFrame = {
  type: "event",
  seq: 7,
  name: "task:created",
  payload: { id: "t1" },
};

describe("environment isolation on the shared relay channel", () => {
  it("delivers frames addressed to this environment", () => {
    const localBus = new MockEventBus();
    const t = target();
    attachRemoteStreamRelay({ localBus, target: t.stub });

    localBus.emit(REMOTE_STREAM_FRAME_EVENT, {
      environmentId: ENV,
      frame: EVENT_FRAME,
    });

    expect(t.frames).toEqual([EVENT_FRAME]);
  });

  it("drops frames addressed to a DIFFERENT environment", () => {
    const localBus = new MockEventBus();
    const t = target();
    attachRemoteStreamRelay({ localBus, target: t.stub });

    localBus.emit(REMOTE_STREAM_FRAME_EVENT, {
      environmentId: OTHER,
      frame: EVENT_FRAME,
    });

    expect(t.frames).toEqual([]);
  });

  it("keeps two environments' relays independent on one channel", () => {
    const localBus = new MockEventBus();
    const a = target(ENV);
    const b = target(OTHER);
    attachRemoteStreamRelay({ localBus, target: a.stub });
    attachRemoteStreamRelay({ localBus, target: b.stub });

    localBus.emit(REMOTE_STREAM_FRAME_EVENT, { environmentId: ENV, frame: EVENT_FRAME });
    localBus.emit(REMOTE_STREAM_FRAME_EVENT, {
      environmentId: OTHER,
      frame: { type: "heartbeat", t: 9 },
    });

    expect(a.frames).toEqual([EVENT_FRAME]);
    expect(b.frames).toEqual([{ type: "heartbeat", t: 9 }]);
  });

  it("routes a close only to the environment it names", () => {
    const localBus = new MockEventBus();
    const a = target(ENV);
    const b = target(OTHER);
    const onStreamClosed = vi.fn();
    attachRemoteStreamRelay({ localBus, target: a.stub, onStreamClosed });
    attachRemoteStreamRelay({ localBus, target: b.stub });

    localBus.emit(REMOTE_STREAM_CLOSED_EVENT, {
      environmentId: ENV,
      reason: "socket ended",
    });

    expect(a.closed).toHaveBeenCalledTimes(1);
    expect(b.closed).not.toHaveBeenCalled();
    expect(onStreamClosed).toHaveBeenCalledWith("socket ended");
  });
});

describe("fail-closed decoding", () => {
  it("drops an undecodable payload and surfaces it", () => {
    const localBus = new MockEventBus();
    const t = target();
    const onUndecodableFrame = vi.fn();
    attachRemoteStreamRelay({ localBus, target: t.stub, onUndecodableFrame });

    localBus.emit(REMOTE_STREAM_FRAME_EVENT, { environmentId: ENV, frame: { type: "??" } });
    localBus.emit(REMOTE_STREAM_FRAME_EVENT, "not an object");

    expect(t.frames).toEqual([]);
    expect(onUndecodableFrame).toHaveBeenCalledTimes(2);
  });

  it("drops a durable frame whose seq is not a number rather than treating it as transient", () => {
    const localBus = new MockEventBus();
    const t = target();
    attachRemoteStreamRelay({ localBus, target: t.stub });

    localBus.emit(REMOTE_STREAM_FRAME_EVENT, {
      environmentId: ENV,
      frame: { type: "event", seq: "7", name: "task:created", payload: {} },
    });

    expect(t.frames).toEqual([]);
  });
});

describe("watchdog and teardown", () => {
  it("reports frame activity only for its own environment (P-9 watchdog input)", () => {
    const localBus = new MockEventBus();
    const t = target();
    const onFrameActivity = vi.fn();
    attachRemoteStreamRelay({ localBus, target: t.stub, onFrameActivity });

    localBus.emit(REMOTE_STREAM_FRAME_EVENT, { environmentId: ENV, frame: EVENT_FRAME });
    localBus.emit(REMOTE_STREAM_FRAME_EVENT, { environmentId: OTHER, frame: EVENT_FRAME });

    expect(onFrameActivity).toHaveBeenCalledTimes(1);
  });

  it("detaches both subscriptions, idempotently", () => {
    const localBus = new MockEventBus();
    const t = target();
    const detach = attachRemoteStreamRelay({ localBus, target: t.stub });
    expect(localBus.getListenerCount(REMOTE_STREAM_FRAME_EVENT)).toBe(1);

    detach();
    detach();

    expect(localBus.getListenerCount(REMOTE_STREAM_FRAME_EVENT)).toBe(0);
    expect(localBus.getListenerCount(REMOTE_STREAM_CLOSED_EVENT)).toBe(0);

    localBus.emit(REMOTE_STREAM_FRAME_EVENT, { environmentId: ENV, frame: EVENT_FRAME });
    expect(t.frames).toEqual([]);
  });
});

describe("wired to a real NetworkEventBus", () => {
  it("projects a relayed frame end to end", async () => {
    const localBus = new MockEventBus();
    const sent: unknown[] = [];
    const bus = new NetworkEventBus({
      environmentId: ENV,
      localBus,
      sendFrame: async (frame) => {
        sent.push(frame);
      },
      hydrate: async () => {},
      sweep: () => {},
      onRestartRequired: () => {},
    });
    attachRemoteStreamRelay({ localBus, target: bus });

    const seen: unknown[] = [];
    bus.subscribe("task:created", (payload) => seen.push(payload));

    await bus.beginStream({
      environmentId: ENV,
      hostEnvironmentId: "host-env-abc",
      streamEpoch: "epoch-1",
      maxSeq: 6,
      heartbeatSecs: 20,
      protocolVersion: 1,
    });
    localBus.emit(REMOTE_STREAM_FRAME_EVENT, { environmentId: ENV, frame: EVENT_FRAME });

    expect(seen).toEqual([{ id: "t1" }]);
    expect(bus.cursor().lastSeq).toBe(7);
  });

  it("another environment's frame never reaches this bus's cursor", async () => {
    const localBus = new MockEventBus();
    const bus = new NetworkEventBus({
      environmentId: ENV,
      localBus,
      sendFrame: async () => {},
      hydrate: async () => {},
      sweep: () => {},
      onRestartRequired: () => {},
    });
    attachRemoteStreamRelay({ localBus, target: bus });
    bus.subscribe("task:created", () => {});

    await bus.beginStream({
      environmentId: ENV,
      hostEnvironmentId: "host-env-abc",
      streamEpoch: "epoch-1",
      maxSeq: 6,
      heartbeatSecs: 20,
      protocolVersion: 1,
    });
    localBus.emit(REMOTE_STREAM_FRAME_EVENT, { environmentId: OTHER, frame: EVENT_FRAME });

    expect(bus.cursor().lastSeq).toBe(6);
  });
});
