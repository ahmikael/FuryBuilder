import { screen } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

/**
 * Chooses an item from a shadcn Select. The trigger and its popup are separate
 * because Base UI renders the list in a portal.
 */
export async function chooseSelectOption(
  user: UserEvent,
  label: string,
  option: string | RegExp,
): Promise<void> {
  await user.click(screen.getByRole("combobox", { name: label }));
  await user.click(await screen.findByRole("option", { name: option }));
}
