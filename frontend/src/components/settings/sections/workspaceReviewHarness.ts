import {
  type KnownHarness,
  KNOWN_HARNESSES,
} from "@/api/ideation-harness";

export function isKnownHarness(value: string): value is KnownHarness {
  return (KNOWN_HARNESSES as readonly string[]).includes(value);
}
