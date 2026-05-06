/**
 * OrbitingAgentNode - Agent node that orbits around a center point
 *
 * Uses CSS animations for smooth 60fps orbital motion (GPU-accelerated).
 * Position is calculated using CSS custom properties and transform.
 *
 * Anti-AI-Slop: Warm colors only, smooth physics-based animation
 */

import { motion } from "framer-motion";
import type { OrbitingAgent } from "./agentConfig";
import { calculateOrbitalRadius } from "./agentConfig";

interface OrbitingAgentNodeProps {
  agent: OrbitingAgent;
  /** Viewport dimensions for calculating orbital radius */
  viewportWidth: number;
  viewportHeight: number;
  /** Center coordinates (typically viewport center) */
  centerX: number;
  centerY: number;
  /** Size of the node in pixels */
  size?: number;
  /** Index for staggered entrance animation */
  index?: number;
}

export default function OrbitingAgentNode({
  agent,
  viewportWidth,
  viewportHeight,
  centerX,
  centerY,
  size = 60,
  index = 0,
}: OrbitingAgentNodeProps) {
  const { name, role, icon: Icon, color, tier, startAngle, period, direction } = agent;

  // Calculate orbital radius based on viewport
  const radius = calculateOrbitalRadius(tier, viewportWidth, viewportHeight);
  const iconSize = size * 0.4;

  // Convert start angle to degrees for CSS
  const startAngleDeg = (startAngle * 180) / Math.PI;

  // Create rgba version of the color for shadows
  const colorToRgba = (hex: string, alpha: number) => {
    const r = parseInt(hex.slice(1, 3), 16);
    const g = parseInt(hex.slice(3, 5), 16);
    const b = parseInt(hex.slice(5, 7), 16);
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  };

  // Unique animation name for this agent
  const animationName = `orbit-${agent.id}`;

  return (
    <>
      {/* CSS keyframe animation for this specific orbit */}
      <style>{`
        @keyframes ${animationName} {
          from {
            transform: translate(-50%, -50%) rotate(${startAngleDeg}deg) translateX(${radius}px) rotate(${-startAngleDeg}deg);
          }
          to {
            transform: translate(-50%, -50%) rotate(${startAngleDeg + direction * 360}deg) translateX(${radius}px) rotate(${-startAngleDeg - direction * 360}deg);
          }
        }
      `}</style>

      <motion.div
        className="absolute flex flex-col items-center pointer-events-auto"
        style={{
          left: centerX,
          top: centerY,
          zIndex: tier === "inner" ? 50 : tier === "middle" ? 40 : 30,
          animation: `${animationName} ${period}s linear infinite`,
          willChange: "transform",
        }}
        // Entrance animation - fade to lower opacity for subtlety
        initial={{ opacity: 0, scale: 0 }}
        animate={{ opacity: 0.6, scale: 1 }}
        transition={{
          type: "spring",
          stiffness: 100,
          damping: 12,
          delay: index * 0.08,
        }}
      >
        {/* Node container with hover effects */}
        <motion.div
          className="relative flex items-center justify-center rounded-full cursor-pointer"
          style={{
            width: size,
            height: size,
            background: `radial-gradient(circle, ${colorToRgba(color, 0.08)} 0%, ${colorToRgba(color, 0.02)} 70%, transparent 100%)`,
            boxShadow: `0 0 8px ${colorToRgba(color, 0.12)}`,
          }}
          whileHover={{
            scale: 1.2,
            boxShadow: `0 0 20px ${colorToRgba(color, 0.4)}, 0 0 35px ${colorToRgba(color, 0.2)}`,
          }}
          whileTap={{ scale: 1.1 }}
          transition={{
            type: "spring",
            stiffness: 400,
            damping: 15,
          }}
        >
          {/* Breathing glow ring - subtle */}
          <motion.div
            className="absolute inset-0 rounded-full"
            style={{
              border: `1px solid ${colorToRgba(color, 0.15)}`,
              boxShadow: `0 0 5px ${colorToRgba(color, 0.08)}`,
            }}
            animate={{
              scale: [1, 1.03, 1],
              boxShadow: [
                `0 0 5px ${colorToRgba(color, 0.08)}`,
                `0 0 10px ${colorToRgba(color, 0.15)}`,
                `0 0 5px ${colorToRgba(color, 0.08)}`,
              ],
            }}
            transition={{
              duration: 3,
              repeat: Infinity,
              ease: "easeInOut",
              delay: index * 0.2,
            }}
          />

          {/* Inner glow layer - subtle */}
          <motion.div
            className="absolute rounded-full"
            style={{
              width: size * 0.7,
              height: size * 0.7,
              background: `radial-gradient(circle, ${colorToRgba(color, 0.12)} 0%, transparent 70%)`,
            }}
            animate={{
              opacity: [0.3, 0.5, 0.3],
            }}
            transition={{
              duration: 2.5,
              repeat: Infinity,
              ease: "easeInOut",
            }}
          />

          {/* Icon container */}
          <div
            className="relative z-10 flex items-center justify-center rounded-full"
            style={{
              width: size * 0.55,
              height: size * 0.55,
              background: `linear-gradient(135deg, ${colorToRgba(color, 0.12)} 0%, ${colorToRgba(color, 0.04)} 100%)`,
              backdropFilter: "blur(3px)",
              border: `1px solid ${colorToRgba(color, 0.18)}`,
            }}
          >
            <Icon
              size={iconSize}
              color={colorToRgba(color, 0.8)}
              strokeWidth={1.5}
              style={{
                filter: `drop-shadow(0 0 3px ${colorToRgba(color, 0.25)})`,
              }}
            />
          </div>
        </motion.div>

        {/* Label - name and role (smaller and more subtle) */}
        <motion.div
          className="mt-2 text-center whitespace-nowrap"
          initial={{ opacity: 0, y: 5 }}
          animate={{ opacity: 0.7, y: 0 }}
          transition={{
            delay: index * 0.08 + 0.2,
            duration: 0.3,
          }}
        >
          <div
            className="text-[0.625rem] font-medium"
            style={{
              color: colorToRgba(color, 0.85),
              textShadow: `0 0 4px ${colorToRgba(color, 0.2)}`,
            }}
          >
            {name}
          </div>
          <div
            className="text-[0.5625rem] mt-0.5"
            style={{ color: "rgba(255, 255, 255, 0.35)" }}
          >
            {role}
          </div>
        </motion.div>
      </motion.div>
    </>
  );
}
