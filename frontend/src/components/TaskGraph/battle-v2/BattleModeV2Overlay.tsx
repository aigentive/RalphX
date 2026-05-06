import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Application, Container, Graphics } from "pixi.js";
import type { TaskGraphNode } from "@/api/task-graph.types";
import {
  BATTLE_V2_QUALITY_PRESETS,
  BATTLE_V2_ROSTER_LIMIT,
  type BattleV2Quality,
} from "./config";
import {
  applyTaskSyncEvent,
  createEngineState,
  getActiveRoster,
  stepState,
  syncTasksSnapshot,
} from "./engine";
import { useBattleTaskFeedV2 } from "./useBattleTaskFeedV2";
import type { BattleEngineState } from "./types";

interface BattleModeV2OverlayProps {
  active: boolean;
  tasks: TaskGraphNode[];
  runningCount: number;
  queuedCount: number;
  onExit: () => void;
}

interface Star {
  x: number;
  y: number;
  speed: number;
  alpha: number;
  size: number;
}

interface PixelExplosion {
  x: number;
  y: number;
  life: number;
  size: number;
  color: number;
  seed: number;
}

function nextQuality(current: BattleV2Quality): BattleV2Quality {
  if (current === "balanced") return "high";
  if (current === "high") return "low";
  return "balanced";
}

function qualityLabel(quality: BattleV2Quality): string {
  if (quality === "high") return "High";
  if (quality === "low") return "Low";
  return "Balanced";
}

function hex(color: string): number {
  if (color.startsWith("#")) {
    return Number.parseInt(color.slice(1), 16);
  }
  return 0xffffff;
}

function drawInvaderGlyph(
  g: Graphics,
  x: number,
  y: number,
  w: number,
  h: number,
  color: number,
  alpha: number,
  kind: "drone" | "elite" | "hazard" | "cluster",
  marchFrame: 0 | 1
) {
  const cell = Math.max(2, Math.floor(Math.min(w / 8, h / 8)));
  const glyphW = cell * 8;
  const glyphH = cell * 8;
  const ox = x + Math.floor((w - glyphW) / 2);
  const oy = y + Math.floor((h - glyphH) / 2);

  const drone = [
    "..xxxx..",
    ".xxxxxx.",
    "xx.xx.xx",
    "xxxxxxxx",
    "x.xxxx.x",
    "x......x",
    ".x....x.",
    "x.x..x.x",
  ];
  const droneMarch = [
    "..xxxx..",
    ".xxxxxx.",
    "xx.xx.xx",
    "xxxxxxxx",
    "x.xxxx.x",
    ".x....x.",
    "x......x",
    ".x.xx.x.",
  ];
  const elite = [
    "..xxxx..",
    ".xxxxxx.",
    "xxxxxxxx",
    "xx.xx.xx",
    "xxxxxxxx",
    ".x.xx.x.",
    "x.x..x.x",
    ".x....x.",
  ];
  const eliteMarch = [
    "..xxxx..",
    ".xxxxxx.",
    "xxxxxxxx",
    "xx.xx.xx",
    "xxxxxxxx",
    "x..xx..x",
    ".x....x.",
    "x.x..x.x",
  ];
  const hazard = [
    "x......x",
    ".x....x.",
    "..x..x..",
    "...xx...",
    "...xx...",
    "..x..x..",
    ".x....x.",
    "x......x",
  ];
  const cluster = [
    ".x....x.",
    "xx.xx.xx",
    "xxxxxxxx",
    "x.xxxx.x",
    "xxxxxxxx",
    ".xxxxxx.",
    "..xxxx..",
    "...xx...",
  ];
  const clusterMarch = [
    ".x....x.",
    "xx.xx.xx",
    "xxxxxxxx",
    "x.xxxx.x",
    ".xxxxxx.",
    "xxxxxxxx",
    "..xxxx..",
    "x.x..x.x",
  ];

  const rows = kind === "elite"
    ? (marchFrame === 0 ? elite : eliteMarch)
    : kind === "hazard"
      ? hazard
      : kind === "cluster"
        ? (marchFrame === 0 ? cluster : clusterMarch)
        : (marchFrame === 0 ? drone : droneMarch);
  for (let ry = 0; ry < rows.length; ry += 1) {
    const row = rows[ry] ?? "";
    for (let rx = 0; rx < row.length; rx += 1) {
      if (row[rx] !== "x") continue;
      g.rect(ox + rx * cell, oy + ry * cell, cell - 0.5, cell - 0.5).fill({ color, alpha });
    }
  }
}

function spawnPixelExplosion(
  list: PixelExplosion[],
  x: number,
  y: number,
  color: number,
  size: number,
  seed: number
) {
  list.push({
    x,
    y,
    life: 1,
    color,
    size,
    seed,
  });
  if (list.length > 40) {
    list.splice(0, list.length - 40);
  }
}

export function BattleModeV2Overlay({
  active,
  tasks,
  runningCount,
  queuedCount,
  onExit,
}: BattleModeV2OverlayProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const appRef = useRef<Application | null>(null);
  const lastTsRef = useRef<number>(0);
  const sizeRef = useRef({ width: 800, height: 600 });
  const keysRef = useRef({ left: false, right: false, firing: false, ability: false });
  const audioCtxRef = useRef<AudioContext | null>(null);
  const musicOscRef = useRef<OscillatorNode | null>(null);
  const musicGainRef = useRef<GainNode | null>(null);
  const musicLeadOscRef = useRef<OscillatorNode | null>(null);
  const musicLeadGainRef = useRef<GainNode | null>(null);
  const starsNearRef = useRef<Star[]>([]);
  const starsFarRef = useRef<Star[]>([]);
  const prevScoreRef = useRef(0);
  const prevLivesRef = useRef(3);
  const prevAbilityRef = useRef(0);
  const bossAliveRef = useRef(false);
  const shakeRef = useRef(0);
  const flashRef = useRef(0);
  const shockwaveRef = useRef<{ x: number; y: number; r: number; life: number } | null>(null);
  const explosionsRef = useRef<PixelExplosion[]>([]);
  const prevSparkCountRef = useRef(0);

  const [soundEnabled, setSoundEnabled] = useState(false);
  const [masterVolume, setMasterVolume] = useState(0.7);
  const [musicVolume, setMusicVolume] = useState(0.45);
  const [sfxVolume, setSfxVolume] = useState(0.65);
  const [quality, setQuality] = useState<BattleV2Quality>("balanced");
  const [hud, setHud] = useState({
    score: 0,
    combo: 0,
    lives: 3,
    overload: 0,
    wave: 1,
    paused: false,
    gameOver: false,
    abilityCharge: 0,
    activeRoster: [] as Array<{ taskId: string; title: string; status: string }>,
    activeCallouts: [] as Array<{ id: string; x: number; y: number; label: string; status: string }>,
  });

  const stateRef = useRef<BattleEngineState>(createEngineState(tasks, 800));

  const allowedTaskIds = useMemo(() => new Set(tasks.map((task) => task.taskId)), [tasks]);

  const playSfx = useCallback((freq: number, durSec: number, volume = 0.03) => {
    if (!soundEnabled) return;
    const ctx = audioCtxRef.current ?? new AudioContext();
    audioCtxRef.current = ctx;

    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "sawtooth";
    osc.frequency.value = freq;
    gain.gain.value = volume * masterVolume * sfxVolume;
    gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + durSec);

    osc.connect(gain);
    gain.connect(ctx.destination);
    osc.start();
    osc.stop(ctx.currentTime + durSec);
  }, [masterVolume, sfxVolume, soundEnabled]);

  useEffect(() => {
    if (!soundEnabled) {
      if (musicOscRef.current) {
        musicOscRef.current.stop();
        musicOscRef.current.disconnect();
        musicOscRef.current = null;
      }
      if (musicGainRef.current) {
        musicGainRef.current.disconnect();
        musicGainRef.current = null;
      }
      if (musicLeadOscRef.current) {
        musicLeadOscRef.current.stop();
        musicLeadOscRef.current.disconnect();
        musicLeadOscRef.current = null;
      }
      if (musicLeadGainRef.current) {
        musicLeadGainRef.current.disconnect();
        musicLeadGainRef.current = null;
      }
      return;
    }

    const ctx = audioCtxRef.current ?? new AudioContext();
    audioCtxRef.current = ctx;

    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "triangle";
    osc.frequency.value = 88;
    gain.gain.value = 0.0001;
    gain.connect(ctx.destination);
    osc.connect(gain);
    osc.start();

    const leadOsc = ctx.createOscillator();
    const leadGain = ctx.createGain();
    leadOsc.type = "sine";
    leadOsc.frequency.value = 176;
    leadGain.gain.value = 0.0001;
    leadGain.connect(ctx.destination);
    leadOsc.connect(leadGain);
    leadOsc.start();

    musicOscRef.current = osc;
    musicGainRef.current = gain;
    musicLeadOscRef.current = leadOsc;
    musicLeadGainRef.current = leadGain;

    const interval = window.setInterval(() => {
      const state = stateRef.current;
      const pressure = Math.min(1, (runningCount * 2 + queuedCount + state.combo * 0.4) / 16);
      const baseFreq = 72 + pressure * 54 + Math.sin(Date.now() / 1200) * 6;
      const leadFreq = 146 + pressure * 140 + Math.sin(Date.now() / 420) * 14;
      osc.frequency.exponentialRampToValueAtTime(baseFreq, ctx.currentTime + 0.25);
      leadOsc.frequency.exponentialRampToValueAtTime(leadFreq, ctx.currentTime + 0.25);
      gain.gain.exponentialRampToValueAtTime(
        Math.max(0.0001, 0.022 * pressure * masterVolume * musicVolume),
        ctx.currentTime + 0.25
      );
      leadGain.gain.exponentialRampToValueAtTime(
        Math.max(0.0001, 0.014 * pressure * masterVolume * musicVolume),
        ctx.currentTime + 0.25
      );
    }, 250);

    return () => {
      window.clearInterval(interval);
      try {
        osc.stop();
        leadOsc.stop();
      } catch {
        // no-op for already stopped node
      }
      osc.disconnect();
      gain.disconnect();
      leadOsc.disconnect();
      leadGain.disconnect();
      if (musicOscRef.current === osc) musicOscRef.current = null;
      if (musicGainRef.current === gain) musicGainRef.current = null;
      if (musicLeadOscRef.current === leadOsc) musicLeadOscRef.current = null;
      if (musicLeadGainRef.current === leadGain) musicLeadGainRef.current = null;
    };
  }, [masterVolume, musicVolume, queuedCount, runningCount, soundEnabled]);

  const syncHud = useCallback(() => {
    const state = stateRef.current;
    const activeCallouts = Array.from(state.entities.values())
      .filter((entity) => entity.active && entity.taskIds.length > 0)
      .sort((a, b) => b.threatWeight - a.threatWeight)
      .slice(0, 6)
      .map((entity) => {
        const taskId = entity.taskIds[0] ?? "";
        const task = taskId ? state.tasks.get(taskId) : undefined;
        const status = task?.status ?? "executing";
        return {
          id: entity.id,
          x: entity.x + entity.w * 0.5,
          y: entity.y - 12,
          label: task?.title ?? entity.label,
          status,
        };
      });

    setHud({
      score: state.score,
      combo: state.combo,
      lives: state.lives,
      overload: state.overload,
      wave: state.wave,
      paused: state.paused,
      gameOver: state.gameOver,
      abilityCharge: state.abilityCharge,
      activeRoster: getActiveRoster(state, BATTLE_V2_ROSTER_LIMIT),
      activeCallouts,
    });
  }, []);

  const reset = useCallback(() => {
    stateRef.current = createEngineState(tasks, sizeRef.current.width);
    explosionsRef.current = [];
    prevSparkCountRef.current = 0;
    syncHud();
  }, [syncHud, tasks]);

  useBattleTaskFeedV2({
    active,
    allowedTaskIds,
    onEvent: useCallback((event) => {
      applyTaskSyncEvent(stateRef.current, event, sizeRef.current.width);
      playSfx(780, 0.04, 0.024);
    }, [playSfx]),
  });

  useEffect(() => {
    if (!active) return;
    reset();
  }, [active, reset]);

  useEffect(() => {
    if (!active) return;
    syncTasksSnapshot(stateRef.current, tasks, sizeRef.current.width);
  }, [active, tasks]);

  useEffect(() => {
    if (!active) return;

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "ArrowLeft" || event.key.toLowerCase() === "a") keysRef.current.left = true;
      if (event.key === "ArrowRight" || event.key.toLowerCase() === "d") keysRef.current.right = true;
      if (event.key === " " || event.key.toLowerCase() === "j") keysRef.current.firing = true;
      if (event.key === "Shift") keysRef.current.ability = true;
      if (event.key.toLowerCase() === "p") {
        stateRef.current.paused = !stateRef.current.paused;
        syncHud();
      }
      if (event.key.toLowerCase() === "f") {
        stateRef.current.focusActiveUntil = Date.now() + 3000;
      }
      if (event.key === "Escape") onExit();
      if (event.key.toLowerCase() === "r" && stateRef.current.gameOver) {
        reset();
      }
    };

    const onKeyUp = (event: KeyboardEvent) => {
      if (event.key === "ArrowLeft" || event.key.toLowerCase() === "a") keysRef.current.left = false;
      if (event.key === "ArrowRight" || event.key.toLowerCase() === "d") keysRef.current.right = false;
      if (event.key === " " || event.key.toLowerCase() === "j") keysRef.current.firing = false;
      if (event.key === "Shift") keysRef.current.ability = false;
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [active, onExit, reset, syncHud]);

  useEffect(() => {
    if (!active || !hostRef.current) return;
    const host = hostRef.current;

    let disposed = false;

    const preset = BATTLE_V2_QUALITY_PRESETS[quality];

    const init = async () => {
      const app = new Application();
      await app.init({
        width: sizeRef.current.width,
        height: sizeRef.current.height,
        antialias: true,
        backgroundAlpha: 0,
        resolution: window.devicePixelRatio || 1,
        autoDensity: true,
      });

      if (disposed) {
        app.destroy(true);
        return;
      }

      appRef.current = app;
      const canvas = app.canvas;
      canvas.style.width = "100%";
      canvas.style.height = "100%";
      host.appendChild(canvas);

      const root = new Container();
      const bgLayer = new Graphics();
      const worldLayer = new Graphics();
      const fxLayer = new Graphics();
      root.addChild(bgLayer);
      root.addChild(worldLayer);
      root.addChild(fxLayer);
      app.stage.addChild(root);
      prevScoreRef.current = stateRef.current.score;
      prevLivesRef.current = stateRef.current.lives;
      prevAbilityRef.current = stateRef.current.abilityCharge;
      bossAliveRef.current = false;
      explosionsRef.current = [];
      prevSparkCountRef.current = 0;

      const createStar = (near: boolean): Star => ({
        x: Math.random() * sizeRef.current.width,
        y: Math.random() * sizeRef.current.height,
        speed: near ? 12 + Math.random() * 16 : 4 + Math.random() * 8,
        alpha: near ? 0.1 + Math.random() * 0.24 : 0.04 + Math.random() * 0.12,
        size: near ? 1 + Math.random() * 2 : 0.4 + Math.random() * 1,
      });

      starsFarRef.current = Array.from({ length: preset.starFar }, () => createStar(false));
      starsNearRef.current = Array.from({ length: preset.starNear }, () => createStar(true));

      const resize = () => {
        if (!hostRef.current) return;
        const rect = hostRef.current.getBoundingClientRect();
        sizeRef.current.width = Math.max(420, rect.width);
        sizeRef.current.height = Math.max(320, rect.height);
        app.renderer.resize(sizeRef.current.width, sizeRef.current.height);
        syncTasksSnapshot(stateRef.current, tasks, sizeRef.current.width);
      };

      resize();
      window.addEventListener("resize", resize);

      const tickerFn = () => {
        const now = Date.now();
        const deltaMs = app.ticker.deltaMS;
        const dt = Math.min(0.05, deltaMs / 1000);

        const state = stateRef.current;
        stepState(
          state,
          dt,
          now,
          sizeRef.current.width,
          sizeRef.current.height,
          keysRef.current,
          runningCount,
          queuedCount
        );
        const scoreDelta = state.score - prevScoreRef.current;
        const livesDelta = prevLivesRef.current - state.lives;
        const sparkCount = state.sparks.length;
        const prevSparkCount = prevSparkCountRef.current;
        const hasBoss = Array.from(state.entities.values()).some((entity) => entity.kind === "miniBoss");
        const bossJustSpawned = !bossAliveRef.current && hasBoss;
        const abilityBlast = prevAbilityRef.current > 95 && state.abilityCharge < 5;
        prevScoreRef.current = state.score;
        prevLivesRef.current = state.lives;
        prevAbilityRef.current = state.abilityCharge;
        bossAliveRef.current = hasBoss;

        if (scoreDelta > 0) {
          shakeRef.current = Math.min(8, shakeRef.current + 0.9);
        }
        if (livesDelta > 0) {
          shakeRef.current = Math.min(20, shakeRef.current + 6);
          flashRef.current = Math.min(1, flashRef.current + 0.45);
        }
        if (bossJustSpawned) {
          shakeRef.current = Math.min(24, shakeRef.current + 9);
          flashRef.current = Math.min(1, flashRef.current + 0.55);
          shockwaveRef.current = {
            x: sizeRef.current.width * 0.5,
            y: 80,
            r: 30,
            life: 1,
          };
          playSfx(220, 0.12, 0.028);
          spawnPixelExplosion(
            explosionsRef.current,
            sizeRef.current.width * 0.5,
            86,
            0xf472b6,
            5,
            Math.floor(now % 997)
          );
        }
        if (abilityBlast) {
          shakeRef.current = Math.min(18, shakeRef.current + 5);
          flashRef.current = Math.min(1, flashRef.current + 0.35);
          shockwaveRef.current = {
            x: state.playerX + 17,
            y: sizeRef.current.height - 58,
            r: 20,
            life: 1,
          };
          playSfx(680, 0.09, 0.03);
          spawnPixelExplosion(
            explosionsRef.current,
            state.playerX + 17,
            sizeRef.current.height - 62,
            0x67e8f9,
            4,
            Math.floor(now % 541)
          );
        }
        if (sparkCount > prevSparkCount) {
          const start = Math.max(prevSparkCount, sparkCount - 6);
          for (let i = start; i < sparkCount; i += 1) {
            const spark = state.sparks[i];
            if (!spark) continue;
            spawnPixelExplosion(
              explosionsRef.current,
              spark.x,
              spark.y,
              hex(spark.color),
              3 + (i % 3),
              i * 13 + Math.floor(now % 389)
            );
          }
        }
        if (livesDelta > 0) {
          spawnPixelExplosion(
            explosionsRef.current,
            state.playerX + 17,
            sizeRef.current.height - 50,
            0xfda4af,
            6,
            Math.floor(now % 733)
          );
        }
        prevSparkCountRef.current = sparkCount;

        bgLayer.clear();
        worldLayer.clear();
        fxLayer.clear();

        shakeRef.current = Math.max(0, shakeRef.current - dt * 14);
        flashRef.current = Math.max(0, flashRef.current - dt * 1.8);
        const shake = shakeRef.current * preset.screenShake;
        root.position.set(
          (Math.random() - 0.5) * shake,
          (Math.random() - 0.5) * shake
        );

        // Keep graph visible under battle mode: translucent "glass" atmosphere instead of opaque sky.
        bgLayer.rect(0, 0, sizeRef.current.width, sizeRef.current.height).fill({ color: 0x020617, alpha: 0.09 });
        bgLayer.rect(0, 0, sizeRef.current.width, sizeRef.current.height).fill({ color: 0x0b1222, alpha: 0.06 });
        bgLayer.rect(0, 0, sizeRef.current.width, sizeRef.current.height).fill({ color: 0x2b0f32, alpha: 0.03 });
        for (let y = 48; y < sizeRef.current.height; y += 56) {
          bgLayer
            .moveTo(0, y + Math.sin(now / 1200 + y * 0.03) * 2)
            .lineTo(sizeRef.current.width, y)
            .stroke({ color: 0x334155, alpha: 0.08, width: 1 });
        }
        for (let x = 64; x < sizeRef.current.width; x += 96) {
          bgLayer
            .moveTo(x + Math.sin(now / 1500 + x * 0.01) * 3, 0)
            .lineTo(x, sizeRef.current.height)
            .stroke({ color: 0x1e293b, alpha: 0.045, width: 1 });
        }
        // Aurora wave bands
        for (let i = 0; i < 3; i += 1) {
          const color = i === 0 ? 0x22d3ee : i === 1 ? 0xa855f7 : 0x60a5fa;
          const alpha = 0.02 + i * 0.014;
          const yBase = sizeRef.current.height * (0.18 + i * 0.12);
          bgLayer.moveTo(0, yBase);
          for (let x = 0; x <= sizeRef.current.width; x += 28) {
            const y = yBase + Math.sin(x / (95 + i * 25) + now / (780 - i * 120)) * (8 + i * 3);
            bgLayer.lineTo(x, y);
          }
          bgLayer.stroke({ color, alpha, width: 2 + i });
        }
        // Energy streaks (angled drift) for kinetic atmosphere
        const streakOffset = (now / 12) % 120;
        for (let x = -180; x < sizeRef.current.width + 180; x += 80) {
          const sx = x + streakOffset;
          bgLayer
            .moveTo(sx, -10)
            .lineTo(sx - 70, sizeRef.current.height + 10)
            .stroke({ color: 0xfb7185, alpha: 0.02, width: 1 });
        }
        // Curved "graph lanes" in the style direction reference
        for (let lane = 0; lane < 7; lane += 1) {
          const lx = 120 + lane * ((sizeRef.current.width - 240) / 6);
          const wobble = Math.sin(now / 900 + lane * 0.8) * 14;
          bgLayer
            .moveTo(lx + wobble, -20)
            .bezierCurveTo(
              lx - 90 + wobble * 0.3,
              sizeRef.current.height * 0.25,
              lx + 95 - wobble * 0.5,
              sizeRef.current.height * 0.62,
              lx - 24,
              sizeRef.current.height + 18
            )
            .stroke({ color: 0xfb7185, alpha: 0.05, width: 1.3 });
        }

        for (const star of starsFarRef.current) {
          star.y += star.speed * dt;
          if (star.y > sizeRef.current.height + 2) star.y = -2;
          bgLayer.rect(star.x, star.y, star.size, star.size).fill({ color: 0xa5b4fc, alpha: star.alpha });
        }

        for (const star of starsNearRef.current) {
          star.y += star.speed * dt;
          if (star.y > sizeRef.current.height + 2) star.y = -2;
          bgLayer.rect(star.x, star.y, star.size, star.size).fill({ color: 0xffffff, alpha: star.alpha });
        }

        const focusActive = state.focusActiveUntil > now;
        const pulse = state.pulseUntil > now ? 1 - (state.pulseUntil - now) / 900 : 0;

        if (pulse > 0) {
          fxLayer
            .roundRect(8, 8, sizeRef.current.width - 16, sizeRef.current.height - 16, 12)
            .stroke({ color: 0xffa050, alpha: 0.5 - pulse * 0.45, width: 2 });
        }

        for (const entity of state.entities.values()) {
          const dim = focusActive && !entity.active && !entity.recent;
          const alpha = dim ? 0.24 : 1;
          const color = hex(entity.color);
          const cx = entity.x + entity.w / 2;
          const cy = entity.y + entity.h / 2;

          if (entity.kind === "miniBoss") {
            worldLayer
              .roundRect(entity.x, entity.y, entity.w, entity.h, 12)
              .fill({ color: 0xff3ec9, alpha: 0.42 * alpha })
              .stroke({ color: 0xfbc0ff, width: 2, alpha });
            worldLayer
              .poly([
                cx, entity.y + 4,
                entity.x + entity.w - 8, cy,
                cx, entity.y + entity.h - 4,
                entity.x + 8, cy,
              ])
              .fill({ color: 0xfdf4ff, alpha: 0.28 * alpha });
            // Boss telegraph rings and beam guides
            const ring = 1 + 0.2 * Math.sin(now / 130);
            fxLayer
              .circle(entity.x + entity.w / 2, entity.y + entity.h / 2, (entity.w * 0.7) * ring)
              .stroke({ color: 0xf9a8d4, alpha: 0.45 * alpha, width: 1.4 });
            fxLayer
              .circle(entity.x + entity.w / 2, entity.y + entity.h / 2, (entity.w * 0.95) * ring)
              .stroke({ color: 0xfb7185, alpha: 0.22 * alpha, width: 1.1 });
            fxLayer
              .moveTo(entity.x + entity.w / 2, entity.y + entity.h)
              .lineTo(entity.x + entity.w / 2, sizeRef.current.height)
              .stroke({ color: 0xfb7185, alpha: 0.18 * alpha, width: 1.2 });
          } else {
            const marchFrame = ((Math.floor(now / 280) + (entity.seed % 2)) % 2) as 0 | 1;
            const marchYOffset = marchFrame === 0 ? 0 : 1.5;
            drawInvaderGlyph(
              worldLayer,
              entity.x,
              entity.y + marchYOffset,
              entity.w,
              entity.h,
              color,
              0.9 * alpha,
              entity.kind === "elite" ? "elite" : entity.kind === "hazard" ? "hazard" : entity.kind === "cluster" ? "cluster" : "drone",
              marchFrame
            );
            worldLayer
              .roundRect(entity.x + 1.5, entity.y + 1.5, entity.w - 3, entity.h - 3, 3)
              .stroke({ color: 0xffffff, alpha: 0.18 * alpha, width: 0.9 });
            fxLayer
              .ellipse(cx, cy, entity.w * 0.55, entity.h * 0.44)
              .stroke({ color, alpha: 0.14 * alpha, width: 1.2 });

            if (entity.active || entity.recent) {
              const ringPulse = 1 + 0.12 * Math.sin(now / 130);
              worldLayer
                .roundRect(
                  entity.x - (ringPulse - 1) * 6 - 2,
                  entity.y - (ringPulse - 1) * 4 - 2,
                  entity.w + (ringPulse - 1) * 12 + 4,
                  entity.h + (ringPulse - 1) * 8 + 4,
                  8
                )
                .stroke({ color: entity.active ? 0xffffff : 0xfef08a, width: 1.6, alpha: 0.9 * alpha });
              fxLayer
                .circle(cx, cy, Math.max(entity.w, entity.h) * 0.85 * ringPulse)
                .stroke({ color: entity.active ? 0x67e8f9 : 0xfde68a, width: 1.2, alpha: 0.62 * alpha });
              fxLayer
                .moveTo(cx, entity.y + entity.h)
                .lineTo(cx, sizeRef.current.height - 72)
                .stroke({ color: entity.active ? 0x22d3ee : 0xfbbf24, width: 0.9, alpha: 0.22 * alpha });
              fxLayer
                .roundRect(entity.x - 2, entity.y - 14, entity.w + 4, 8, 2)
                .fill({ color: entity.active ? 0x22d3ee : 0xfde68a, alpha: 0.24 * alpha });
            }
          }
        }

        for (const bullet of state.bullets) {
          const bulletColor = hex(bullet.color);
          const bw = bullet.fromEnemy ? 4 : 3;
          const bh = bullet.fromEnemy ? 12 : 16;
          worldLayer.rect(bullet.x - bw * 0.5, bullet.y, bw, bh).fill({ color: bulletColor, alpha: 0.96 });
          worldLayer.rect(bullet.x - 0.5, bullet.y - 5, 1, 5).fill({ color: 0xffffff, alpha: 0.3 });
          fxLayer
            .moveTo(bullet.x, bullet.y + bh)
            .lineTo(bullet.x - bullet.vx * 0.02, bullet.y + bh + (bullet.fromEnemy ? -8 : 10))
            .stroke({ color: bulletColor, alpha: 0.52, width: 1.4 });
        }

        for (const spark of state.sparks) {
          fxLayer.rect(spark.x, spark.y, 2, 2).fill({ color: hex(spark.color), alpha: Math.max(0, spark.life) });
          fxLayer
            .circle(spark.x, spark.y, (1 - spark.life) * 14 + 2)
            .stroke({ color: hex(spark.color), alpha: Math.max(0, spark.life * 0.25), width: 1 });
          fxLayer
            .moveTo(spark.x, spark.y)
            .lineTo(spark.x - spark.vx * 0.02, spark.y - spark.vy * 0.02)
            .stroke({ color: hex(spark.color), alpha: Math.max(0, spark.life * 0.5), width: 1 });
        }

        for (const explosion of explosionsRef.current) {
          const age = 1 - explosion.life;
          const alpha = Math.max(0, explosion.life);
          const radius = (6 + explosion.size * 2) * (0.35 + age * 1.8);
          for (let i = 0; i < 14; i += 1) {
            const angle = ((i * 25 + explosion.seed * 19) % 360) * (Math.PI / 180);
            const px = explosion.x + Math.cos(angle) * (radius + (i % 3) * 2.5);
            const py = explosion.y + Math.sin(angle) * (radius + ((i + 1) % 3) * 2.5);
            const chunk = i % 2 === 0 ? 3 : 2;
            fxLayer.rect(px, py, chunk, chunk).fill({ color: explosion.color, alpha: 0.8 * alpha });
          }
          fxLayer
            .rect(explosion.x - 2, explosion.y - 2, 4, 4)
            .fill({ color: 0xffffff, alpha: 0.75 * alpha });
          explosion.life -= dt * 2.4;
        }
        explosionsRef.current = explosionsRef.current.filter((explosion) => explosion.life > 0);

        // Shockwave pulse (boss spawn / ability discharge)
        if (shockwaveRef.current) {
          const sw = shockwaveRef.current;
          sw.r += dt * 320;
          sw.life -= dt * 1.25;
          fxLayer
            .circle(sw.x, sw.y, sw.r)
            .stroke({ color: 0xe9d5ff, alpha: Math.max(0, sw.life * 0.45), width: 2 });
          if (sw.life <= 0) {
            shockwaveRef.current = null;
          }
        }

        const py = sizeRef.current.height - 48;
        const playerColor = state.gameOver ? 0x64748b : 0x7dd3fc;
        worldLayer.poly([
          state.playerX + 17, py,
          state.playerX, py + 20,
          state.playerX + 34, py + 20,
        ]).fill({ color: playerColor, alpha: 1 });
        worldLayer
          .poly([
            state.playerX + 17, py + 3,
            state.playerX + 8, py + 18,
            state.playerX + 26, py + 18,
          ])
          .fill({ color: 0xffffff, alpha: 0.22 });
        // Engine bloom
        fxLayer
          .ellipse(state.playerX + 17, py + 24, 12 + Math.sin(now / 90) * 2, 7 + Math.sin(now / 70) * 1.8)
          .fill({ color: 0x67e8f9, alpha: 0.24 });
        fxLayer
          .ellipse(state.playerX + 17, py + 30, 5 + Math.sin(now / 80), 4 + Math.sin(now / 65))
          .fill({ color: 0xfef3c7, alpha: 0.34 });

        for (let y = 0; y < sizeRef.current.height; y += 4) {
          fxLayer.rect(0, y, sizeRef.current.width, 1).fill({ color: 0x94a3b8, alpha: preset.bgStreakAlpha * 0.7 });
        }
        fxLayer.rect(0, 0, sizeRef.current.width, sizeRef.current.height).stroke({
          color: 0x60a5fa,
          alpha: 0.12,
          width: 1.2,
        });
        if (flashRef.current > 0) {
          fxLayer
            .rect(0, 0, sizeRef.current.width, sizeRef.current.height)
            .fill({ color: 0xffffff, alpha: Math.min(0.32, flashRef.current * 0.22) });
        }

        if (state.shotCooldown > 0 && keysRef.current.firing) {
          playSfx(420 + Math.random() * 40, 0.02, 0.015);
        }
        if (state.gameOver) {
          playSfx(140, 0.09, 0.01);
        }

        syncHud();
      };

      app.ticker.add(tickerFn);

      return () => {
        app.ticker.remove(tickerFn);
        window.removeEventListener("resize", resize);
      };
    };

    let cleanupInner: (() => void) | undefined;
    void init().then((cleanup) => {
      cleanupInner = cleanup;
    });

    return () => {
      disposed = true;
      if (cleanupInner) cleanupInner();
      const app = appRef.current;
      if (app) {
        app.destroy(true);
        appRef.current = null;
      }
      host.innerHTML = "";
      lastTsRef.current = 0;
    };
  }, [active, queuedCount, quality, runningCount, soundEnabled, syncHud, playSfx, tasks]);

  useEffect(() => {
    return () => {
      if (musicOscRef.current) {
        try {
          musicOscRef.current.stop();
        } catch {
          // no-op
        }
      }
      musicOscRef.current = null;
      if (musicGainRef.current) {
        musicGainRef.current.disconnect();
      }
      musicGainRef.current = null;
      if (musicLeadOscRef.current) {
        try {
          musicLeadOscRef.current.stop();
        } catch {
          // no-op
        }
      }
      musicLeadOscRef.current = null;
      if (musicLeadGainRef.current) {
        musicLeadGainRef.current.disconnect();
      }
      musicLeadGainRef.current = null;
      if (audioCtxRef.current) {
        audioCtxRef.current.close().catch(() => undefined);
      }
    };
  }, []);

  if (!active) return null;

  return (
    <div className="absolute inset-0 z-20" data-testid="battle-mode-v2-overlay">
      <div ref={hostRef} className="absolute inset-0" />

      <div className="absolute top-3 left-3 flex items-center gap-2 rounded-md border border-cyan-200/20 bg-slate-950/45 px-3 py-2 text-xs text-slate-100">
        <span className="font-semibold text-cyan-300">Neon Rift: Taskfall</span>
        <span className="text-slate-500">|</span>
        <span>Score {hud.score}</span>
        <span className="text-slate-500">|</span>
        <span>Combo x{hud.combo}</span>
        <span className="text-slate-500">|</span>
        <span>Lives {hud.lives}</span>
        <span className="text-slate-500">|</span>
        <span>Wave {hud.wave}</span>
        <span className="text-slate-500">|</span>
        <span className={hud.overload > 65 ? "text-rose-300" : "text-emerald-300"}>
          {hud.overload > 65 ? "Critical Pressure" : "Stable"}
        </span>
      </div>

      {hud.activeCallouts.map((callout) => (
        <div
          key={callout.id}
          className="pointer-events-none absolute -translate-x-1/2 rounded border border-cyan-300/35 bg-slate-950/50 px-2 py-1 text-[0.625rem] text-cyan-100 shadow-[0_0_18px_rgba(34,211,238,0.2)]"
          style={{
            left: callout.x,
            top: Math.max(8, callout.y),
          }}
        >
          <span className="font-semibold">{callout.status}</span>
          <span className="mx-1 text-cyan-300/60">|</span>
          <span>{callout.label.slice(0, 22)}</span>
        </div>
      ))}

      <div className="absolute top-3 left-1/2 -translate-x-1/2 rounded-md border border-white/10 bg-slate-950/42 px-3 py-2 text-xs text-slate-100 max-w-[62%] truncate">
        <span className="text-amber-200">Active Now</span>
        <span className="mx-2 text-slate-500">|</span>
        {hud.activeRoster.length === 0
          ? <span className="text-slate-400">No active tasks</span>
          : <span>{hud.activeRoster.map((entry) => `${entry.taskId.slice(0, 8)}:${entry.status}`).join("  •  ")}</span>}
      </div>

      <div className="absolute top-3 right-3 flex items-center gap-2">
        <button
          type="button"
          className="rounded-md border border-white/15 bg-slate-950/45 px-3 py-2 text-xs text-slate-100 hover:bg-slate-900/70"
          onClick={() => setQuality((prev) => nextQuality(prev))}
          data-testid="battle-v2-quality"
        >
          Quality: {qualityLabel(quality)}
        </button>
        <button
          type="button"
          className="rounded-md border border-white/15 bg-slate-950/45 px-3 py-2 text-xs text-slate-100 hover:bg-slate-900/70"
          onClick={() => setSoundEnabled((prev) => !prev)}
          data-testid="battle-v2-sound"
        >
          {soundEnabled ? "Sound On" : "Sound Off"}
        </button>
        <button
          type="button"
          className="rounded-md border border-white/15 bg-slate-950/45 px-3 py-2 text-xs text-slate-100 hover:bg-slate-900/70"
          onClick={() => {
            stateRef.current.focusActiveUntil = Date.now() + 3000;
          }}
          data-testid="battle-v2-focus"
        >
          Focus Active
        </button>
        <button
          type="button"
          className="rounded-md border border-orange-400/35 bg-orange-500/20 px-3 py-2 text-xs text-orange-100 hover:bg-orange-500/35"
          onClick={onExit}
          data-testid="battle-v2-exit"
        >
          Exit Battle
        </button>
      </div>

      <div className="absolute bottom-3 left-3 rounded-md border border-white/10 bg-slate-950/45 px-3 py-2 text-xs text-slate-200">
        Move: A/D or Arrows | Fire: Space/J | Ability: Shift | Focus Active: F | Pause: P | Exit: Esc
      </div>

      <div className="absolute bottom-3 right-3 rounded-md border border-white/10 bg-slate-950/45 px-3 py-2 text-xs text-slate-200 min-w-[220px]">
        <div className="mb-1 flex items-center justify-between">
          <span>Ability</span>
          <span>{Math.floor(hud.abilityCharge)}%</span>
        </div>
        <div className="h-2 rounded bg-slate-800 overflow-hidden">
          <div className="h-full bg-cyan-400 transition-all" style={{ width: `${hud.abilityCharge}%` }} />
        </div>
        <div className="mt-2 mb-1 flex items-center justify-between">
          <span>Overload</span>
          <span>{Math.floor(hud.overload)}%</span>
        </div>
        <div className="h-2 rounded bg-slate-800 overflow-hidden">
          <div className="h-full bg-rose-400 transition-all" style={{ width: `${hud.overload}%` }} />
        </div>
        <div className="mt-2 mb-1 flex items-center justify-between">
          <span>Threat Load</span>
          <span>{Math.min(100, Math.round(runningCount * 11 + queuedCount * 3 + hud.combo * 1.5))}%</span>
        </div>
        <div className="h-2 rounded bg-slate-800 overflow-hidden">
          <div
            className="h-full bg-gradient-to-r from-cyan-400 via-fuchsia-400 to-rose-400 transition-all"
            style={{ width: `${Math.min(100, runningCount * 11 + queuedCount * 3 + hud.combo * 1.5)}%` }}
          />
        </div>
        <div className="mt-3 space-y-2">
          <label className="flex items-center justify-between gap-2">
            <span>Master</span>
            <input
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={masterVolume}
              onChange={(event) => setMasterVolume(Number(event.target.value))}
              className="w-28"
            />
          </label>
          <label className="flex items-center justify-between gap-2">
            <span>Music</span>
            <input
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={musicVolume}
              onChange={(event) => setMusicVolume(Number(event.target.value))}
              className="w-28"
            />
          </label>
          <label className="flex items-center justify-between gap-2">
            <span>SFX</span>
            <input
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={sfxVolume}
              onChange={(event) => setSfxVolume(Number(event.target.value))}
              className="w-28"
            />
          </label>
        </div>
      </div>

      {hud.gameOver && (
        <div className="absolute inset-0 flex items-center justify-center">
          <div className="rounded-lg border border-rose-300/40 bg-slate-950/85 px-7 py-6 text-center text-slate-100">
            <p className="text-xl font-semibold text-rose-200">System Overrun</p>
            <p className="mt-2 text-sm">Score: {hud.score}</p>
            <p className="mt-2 text-xs text-slate-300">Press R to restart or Exit Battle</p>
          </div>
        </div>
      )}
    </div>
  );
}
