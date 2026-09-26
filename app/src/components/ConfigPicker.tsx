import type {
  SessionConfigOption,
  SessionConfigSelectGroup,
  SessionConfigSelectOption,
} from "@agentclientprotocol/sdk";
import { useEffect, useRef, useState } from "react";

/** A value to send in `set_config_option`. */
export type ConfigValue = { type: "boolean"; value: boolean } | { value: string };

/** Lists longer than this get a filter field. */
const FILTER_AFTER = 8;

/**
 * The editor control for one config option: a button that opens the list of
 * a select option's values, or a toggle for a boolean option.
 */
export function ConfigPicker({
  option,
  onChange,
}: {
  option: SessionConfigOption;
  onChange: (value: ConfigValue) => void;
}) {
  if (option.type === "boolean") {
    return (
      <button
        className={"picker toggle" + (option.currentValue ? " on" : "")}
        title={option.description ?? undefined}
        onClick={() => onChange({ type: "boolean", value: !option.currentValue })}
      >
        {option.name}
      </button>
    );
  }
  return <SelectPicker option={option} onChange={onChange} />;
}

function SelectPicker({
  option,
  onChange,
}: {
  option: Extract<SessionConfigOption, { type: "select" }>;
  onChange: (value: ConfigValue) => void;
}) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const container = useRef<HTMLDivElement>(null);
  const groups = valueGroups(option.options);
  const values = groups.flatMap((group) => group.options);
  const current = values.find((value) => value.value === option.currentValue);

  // A click outside the picker closes its list.
  useEffect(() => {
    if (!open) {
      return;
    }
    const onMouseDown = (event: MouseEvent) => {
      if (!container.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", onMouseDown);
    return () => window.removeEventListener("mousedown", onMouseDown);
  }, [open]);

  const choose = (value: SessionConfigSelectOption) => {
    setOpen(false);
    setFilter("");
    onChange({ value: value.value });
  };

  const query = filter.toLowerCase();
  return (
    <div ref={container} className="picker-container">
      <button
        className="picker"
        title={option.description ?? option.name}
        onClick={() => setOpen(!open)}
      >
        {current?.name ?? option.currentValue} <span className="chevron">⌄</span>
      </button>
      {open && (
        <div className="popover picker-list">
          {values.length > FILTER_AFTER && (
            <input
              className="picker-filter"
              placeholder="Filter"
              autoFocus
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              onKeyDown={(event) => event.key === "Escape" && setOpen(false)}
            />
          )}
          {groups.map((group, index) => {
            const shown = group.options.filter((value) =>
              value.name.toLowerCase().includes(query),
            );
            if (shown.length === 0) {
              return null;
            }
            return (
              <div key={group.group ?? index}>
                {group.name !== undefined && <div className="picker-group">{group.name}</div>}
                {shown.map((value) => (
                  <button
                    key={value.value}
                    className={"picker-value" + (value === current ? " current" : "")}
                    onClick={() => choose(value)}
                  >
                    <span className="check">{value === current ? "✓" : ""}</span>
                    <span className="picker-value-text">
                      <span>{value.name}</span>
                      {value.description != null && (
                        <span className="dim">{value.description}</span>
                      )}
                    </span>
                  </button>
                ))}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** The values as groups; ungrouped values are one group without a name. */
function valueGroups(
  options: SessionConfigSelectOption[] | SessionConfigSelectGroup[],
): { group?: string; name?: string; options: SessionConfigSelectOption[] }[] {
  if (options.length > 0 && "group" in options[0]) {
    return options as SessionConfigSelectGroup[];
  }
  return [{ options: options as SessionConfigSelectOption[] }];
}
