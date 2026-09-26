import type { UsageUpdate } from "@agentclientprotocol/sdk";
import { useState } from "react";
import { usageText } from "../usage";

const RADIUS = 6;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

/**
 * A ring filled by the share of the context in use. Hovering shows the
 * tokens used of the context size and, when reported, the cost.
 */
export function UsageIndicator({ usage }: { usage: UsageUpdate }) {
  const [hovered, setHovered] = useState(false);
  const fraction = usage.size > 0 ? Math.min(usage.used / usage.size, 1) : 0;
  const text = usageText(usage);
  return (
    <div
      className="usage"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <svg width="16" height="16" viewBox="0 0 16 16">
        <circle className="usage-track" cx="8" cy="8" r={RADIUS} />
        <circle
          className="usage-fill"
          cx="8"
          cy="8"
          r={RADIUS}
          strokeDasharray={CIRCUMFERENCE}
          strokeDashoffset={CIRCUMFERENCE * (1 - fraction)}
          transform="rotate(-90 8 8)"
        />
      </svg>
      {hovered && (
        <div className="popover usage-popover">
          <div>{text.tokens}</div>
          {text.cost !== null && <div className="dim">{text.cost}</div>}
        </div>
      )}
    </div>
  );
}
