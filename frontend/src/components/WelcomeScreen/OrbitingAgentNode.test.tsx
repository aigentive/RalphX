import { describe, it, expect, vi } from "vitest";
import { render } from "@testing-library/react";
import { Star } from "lucide-react";

import OrbitingAgentNode from "./OrbitingAgentNode";
import type { OrbitingAgent } from "./agentConfig";

vi.mock("framer-motion", async () => {
  const actual = await vi.importActual<typeof import("framer-motion")>("framer-motion");
  return actual;
});

function makeAgent(overrides: Partial<OrbitingAgent> = {}): OrbitingAgent {
  return {
    id: "agent-1",
    name: "Scout",
    role: "Researcher",
    icon: Star,
    color: "#ff6b35",
    tier: "inner",
    startAngle: 0,
    period: 12,
    direction: 1,
    ...overrides,
  } as OrbitingAgent;
}

describe("OrbitingAgentNode", () => {
  it("renders an agent at the inner tier with default size", () => {
    const { container } = render(
      <OrbitingAgentNode
        agent={makeAgent()}
        viewportWidth={800}
        viewportHeight={600}
        centerX={400}
        centerY={300}
      />,
    );
    expect(container.querySelector("svg")).toBeInTheDocument();
  });

  it("renders agents at outer tier with custom size + counter-clockwise direction", () => {
    const { container } = render(
      <OrbitingAgentNode
        agent={makeAgent({ tier: "outer", direction: -1 })}
        viewportWidth={1200}
        viewportHeight={900}
        centerX={600}
        centerY={450}
        size={80}
        index={3}
      />,
    );
    expect(container.querySelector("svg")).toBeInTheDocument();
  });
});
