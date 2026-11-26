import { Input } from "@/components/ui/input";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { CatalogStates } from "@/assets/types";

const GAME_DEFAULT = "__game_default__";

export type CatalogStateSchema = Pick<
  CatalogStates,
  "properties" | "propertyValuesComplete" | "multipart"
>;

interface CatalogStatePickerProps {
  schema: CatalogStateSchema;
  values: Record<string, string>;
  onChange: (next: Record<string, string>) => void;
  /** True only for a client-JAR blockstate schema; curated quick picks are not exact. */
  exact: boolean;
}

function optionLabel(
  value: string,
  current: string | undefined,
  declared: readonly string[],
): string {
  if (value === GAME_DEFAULT) return "Game default";
  if (value === current && !declared.includes(value)) return `${value} (not declared)`;
  return value;
}

/**
 * Constrained blockstate picker. Complete blockstate files render a select with every
 * declared value; incomplete files keep a text input because undeclared values are valid.
 * An imported value outside the declared set stays visible and is flagged, never dropped.
 */
export function CatalogStatePicker({ schema, values, onChange, exact }: CatalogStatePickerProps) {
  const keys = Array.from(new Set([...Object.keys(schema.properties), ...Object.keys(values)]));
  const undeclared = keys.filter((key) => {
    const current = values[key];
    return current !== undefined && !(schema.properties[key] ?? []).includes(current);
  });

  const update = (key: string, value: string | null | undefined) => {
    const next = { ...values };
    if (value === undefined || value === null || value === "") {
      delete next[key];
    } else {
      next[key] = value;
    }
    onChange(next);
  };

  if (keys.length === 0) {
    return (
      <p className="text-xs text-muted-foreground">
        {schema.multipart
          ? "No single JAR default: multipart states are assembled by the game."
          : "The blockstate file declares no properties; the game default is used."}
      </p>
    );
  }

  return (
    <FieldGroup className="gap-2">
      {keys.map((key) => {
        const declared = schema.properties[key] ?? [];
        const current = values[key];
        const complete = exact && schema.propertyValuesComplete && declared.length > 0;
        const options = [GAME_DEFAULT, ...declared];
        if (current !== undefined && !declared.includes(current)) options.push(current);

        return (
          <Field key={key} orientation="horizontal" className="items-center">
            <FieldLabel htmlFor={`catalog-property-${key}`} className="w-24 shrink-0 truncate">
              <span className="font-mono text-xs text-muted-foreground">{key}</span>
            </FieldLabel>
            {complete ? (
              <Select
                items={options.map((value) => ({
                  value,
                  label: optionLabel(value, current, declared),
                }))}
                value={current ?? GAME_DEFAULT}
                onValueChange={(value) =>
                  update(key, value === GAME_DEFAULT || value == null ? undefined : String(value))
                }
              >
                <SelectTrigger
                  id={`catalog-property-${key}`}
                  size="sm"
                  className="w-full min-w-0 flex-1"
                  aria-label={`Property ${key}`}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent alignItemWithTrigger={false}>
                  <SelectGroup>
                    {options.map((value) => (
                      <SelectItem
                        key={value}
                        value={value}
                        disabled={value === current && !declared.includes(value)}
                      >
                        {optionLabel(value, current, declared)}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            ) : (
              <>
                <Input
                  id={`catalog-property-${key}`}
                  className="h-8 min-w-0 flex-1"
                  value={current ?? ""}
                  placeholder={declared.length > 0 ? declared.join(" | ") : "game default"}
                  list={`catalog-property-${key}-values`}
                  aria-label={`Property ${key}`}
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(event) => update(key, event.target.value)}
                />
                <datalist id={`catalog-property-${key}-values`}>
                  {declared.map((value) => (
                    <option key={value} value={value} />
                  ))}
                </datalist>
              </>
            )}
          </Field>
        );
      })}

      {!exact ? (
        <p className="text-[11px] leading-relaxed text-muted-foreground">
          Curated quick-pick properties are not an exact blockstate schema. Use the client JAR
          catalog for checked values.
        </p>
      ) : null}
      {exact && !schema.propertyValuesComplete ? (
        <p className="text-[11px] leading-relaxed text-muted-foreground">
          This blockstate file declares only part of the state space. Declared keys are checked;
          undeclared values are accepted.
        </p>
      ) : null}
      {schema.multipart ? (
        <p className="text-[11px] leading-relaxed text-muted-foreground">
          Multipart states are assembled by the game; an empty selection uses its default state.
        </p>
      ) : null}
      {undeclared.length > 0 ? (
        <p className="text-[11px] leading-relaxed text-muted-foreground">
          Imported {undeclared.length === 1 ? "value" : "values"}{" "}
          {undeclared.map((key) => `${key}=${values[key]}`).join(", ")}{" "}
          {undeclared.length === 1 ? "is" : "are"} not declared by this blockstate.{" "}
          {undeclared.length === 1 ? "It stays" : "They stay"} visible and{" "}
          {undeclared.length === 1 ? "is" : "are"} never substituted.
        </p>
      ) : null}
    </FieldGroup>
  );
}
