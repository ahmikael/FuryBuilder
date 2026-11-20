import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RecoveryBanner } from "@/features/project/RecoveryBanner";
import type { RecoveryInfo, SessionStatus } from "@/project/types";
import { useProjectStore } from "@/state/project-store";

const session: SessionStatus = {
  folder: "Crashy.furybuilder",
  name: "Crashy",
  savedRevision: 4,
  autosavedRevision: 6,
  revision: 6,
  dirty: true,
  editorState: null,
  availableCheckpoints: 0,
};

const available: RecoveryInfo = {
  reason: "A newer autosave passed checksum and structural verification.",
  recoveredRevision: 6,
  recoveredAtMs: 1_700_000_100_000,
  fromBackup: false,
  available: true,
  autosaveRevision: 6,
};

const unavailable: RecoveryInfo = {
  ...available,
  reason: "The crash autosave does not match its recorded checksum.",
  recoveredRevision: 4,
  available: false,
  autosaveRevision: 9,
};

const original = useProjectStore.getState();

describe("RecoveryBanner", () => {
  beforeEach(() => {
    useProjectStore.setState({
      recovery: null,
      current: null,
      save: original.save,
      discardRecovery: original.discardRecovery,
      dismissRecovery: original.dismissRecovery,
    });
  });

  afterEach(() => {
    // Unmount before resetting the shared store so the reset cannot update a
    // still-mounted banner outside React's act scope.
    cleanup();
    useProjectStore.setState({
      recovery: null,
      current: null,
      save: original.save,
      discardRecovery: original.discardRecovery,
      dismissRecovery: original.dismissRecovery,
    });
  });

  it("renders nothing without a recovery report", () => {
    const { container } = render(<RecoveryBanner />);
    expect(container).toBeEmptyDOMElement();
  });

  it("offers save and discard for a verified recovery", async () => {
    const user = userEvent.setup();
    const save = vi.fn().mockResolvedValue(true);
    const discard = vi.fn().mockResolvedValue(true);
    useProjectStore.setState({
      recovery: available,
      current: session,
      save,
      discardRecovery: discard,
    });

    render(<RecoveryBanner />);
    expect(screen.getByRole("alert", { name: "Crash recovery" })).toHaveTextContent(
      "Recovered unsaved changes",
    );
    expect(screen.getByText(/Autosave revision 6/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Save now" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole("button", { name: "Save now" })).toBeEnabled());

    await user.click(screen.getByRole("button", { name: "Discard recovered changes" }));
    await waitFor(() => expect(discard).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Discard recovered changes" })).toBeEnabled(),
    );
  });

  it("explains an unverified autosave without offering to save it", async () => {
    const user = userEvent.setup();
    useProjectStore.setState({ recovery: unavailable, current: session, save: vi.fn() });

    render(<RecoveryBanner />);
    expect(screen.getByText("Crash recovery could not be applied")).toBeInTheDocument();
    expect(screen.getByText(/does not match its recorded checksum/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save now" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Discard recovered changes" }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(useProjectStore.getState().recovery).toBeNull();
  });
});
