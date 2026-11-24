import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";

export interface ComponentPickerOption<T extends string> {
  value: T;
  label: string;
}

export interface ComponentPickerProps<T extends string> {
  value: T;
  options: readonly ComponentPickerOption<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
  label: string;
  className?: string;
}

/**
 * Shared Auto / Existing / Manual style selector. Project Settings and the creation wizard
 * can offer different options, but they use the same keyboard-accessible shadcn toggle group.
 */
export function ComponentPicker<T extends string>({
  value,
  options,
  onChange,
  disabled,
  label,
  className,
}: ComponentPickerProps<T>) {
  return (
    <ToggleGroup
      variant="outline"
      size="sm"
      value={[value]}
      disabled={disabled}
      aria-label={label}
      className={cn("grid w-full grid-cols-1 sm:grid-cols-3", className)}
      onValueChange={(values) => {
        const next = values[0];
        if (options.some((option) => option.value === next)) {
          onChange(next as T);
        }
      }}
    >
      {options.map((option) => (
        <ToggleGroupItem key={option.value} value={option.value}>
          {option.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
