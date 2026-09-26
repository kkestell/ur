import type {
  SessionConfigOption,
  SessionConfigSelectGroup,
  SessionConfigSelectOption,
} from "@agentclientprotocol/sdk";
import { useRef, useState } from "react";
import { Popover, useClickOutside } from "./Popover";
import { Check, ChevronDown, Image } from "lucide-react";

/** A value to send in `set_config_option`. */
export type ConfigValue = { type: "boolean"; value: boolean } | { value: string };

/** The description Ox gives each model that accepts images. */
const ACCEPTS_IMAGES = "Accepts images";

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
        className={"picker toggle max-w-full min-w-0 truncate rounded px-2 py-1 hover:bg-control" + (option.currentValue ? " on bg-control text-fg" : " text-fg-dim")}
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
  const button = useRef<HTMLButtonElement>(null);
  const groups = valueGroups(option.options);
  const values = groups.flatMap((group) => group.options);
  const current = values.find((value) => value.value === option.currentValue);

  const insideProps = useClickOutside(open, () => setOpen(false));

  const choose = (value: SessionConfigSelectOption) => {
    setOpen(false);
    setFilter("");
    onChange({ value: value.value });
  };

  const query = filter.toLowerCase();
  return (
    <div {...insideProps} className="picker-container min-w-0 max-w-48">
      <button
        ref={button}
        className="picker flex max-w-full min-w-0 items-center gap-1 rounded px-2 py-1 hover:bg-control"
        title={[`${option.name}: ${current?.name ?? option.currentValue}`, option.description].filter(Boolean).join(" — ")}
        onClick={() => setOpen(!open)}
      >
        <span className="min-w-0 truncate">{current?.name ?? option.currentValue}</span>
        <ChevronDown className="chevron shrink-0 text-fg-dim" size={16} strokeWidth={1.75} />
      </button>
      {open && (
        <Popover
          anchor={button}
          align="end"
          className="picker-list max-h-80 min-w-60 max-w-[min(320px,calc(100vw-16px))] overflow-y-auto p-1"
        >
          {values.length > FILTER_AFTER && (
            <input
              className="picker-filter mb-1 w-full border-b border-outline bg-transparent px-2 py-1.5 outline-none"
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
                {group.name !== undefined && <div className="picker-group px-2 pt-1.5 pb-0.5 text-label text-fg-dim">{group.name}</div>}
                {shown.map((value) => (
                  <button
                    key={value.value}
                    className={"picker-value flex w-full items-start gap-2 rounded px-2 py-1.5 text-left hover:bg-control" + (value === current ? " current bg-control" : "")}
                    onClick={() => choose(value)}
                  >
                    <span className="check flex h-lh w-4 shrink-0 items-center justify-center">{value === current && <Check size={14} strokeWidth={1.75} />}</span>
                    <span className="picker-value-text flex min-w-0 flex-1 flex-col">
                      <span>{value.name}</span>
                      {value.description != null && value.description !== ACCEPTS_IMAGES && (
                        <span className="dim text-fg-dim">{value.description}</span>
                      )}
                    </span>
                    {value.description === ACCEPTS_IMAGES && (
                      <span className="accepts-images flex h-lh shrink-0 items-center text-fg-dim" title={ACCEPTS_IMAGES}>
                        <Image size={14} strokeWidth={1.75} />
                      </span>
                    )}
                  </button>
                ))}
              </div>
            );
          })}
        </Popover>
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
