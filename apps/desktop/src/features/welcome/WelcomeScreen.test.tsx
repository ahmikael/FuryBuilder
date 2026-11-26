import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WelcomeScreen } from "@/features/welcome/WelcomeScreen";
import type { BackendState } from "@/hooks/use-backend";
import type { ProjectSummary } from "@/project/types";
import { useProjectStore } from "@/state/project-store";
import { clearToasts, getToasts } from "@/test-utils/toast-log";

const online: BackendState = {
  status: "online",
  info: { name: "FuryBuilder", version: "0.2.0" },
  error: null,
};

const first: ProjectSummary = {
  folder: "First Castle.furybuilder",
  projectId: "project-1",
  name: "First Castle",
  creationMode: "flat",
  targetMinecraftVersion: "1.21.4",
  targetProfileId: "java-classic-1.21.4",
  targetDataVersion: 4189,
  worldRevision: 4,
  updatedAtMs: 1_700_000_000_000,
  lastSavedAtMs: 1_700_000_000_000,
  lastSaveKind: "manual",
  bytes: 2048,
  schemaVersion: 2,
  recoverable: false,
};

const second: ProjectSummary = {
  folder: "Second Base.furybuilder",
  projectId: "project-2",
  name: "Second Base",
  creationMode: "empty",
  targetMinecraftVersion: "1.21.4",
  targetProfileId: "java-classic-1.21.4",
  targetDataVersion: 4189,
  worldRevision: 1,
  updatedAtMs: 1_700_000_100_000,
  lastSavedAtMs: 1_700_000_100_000,
  lastSaveKind: "manual",
  bytes: 512,
  schemaVersion: 2,
  recoverable: true,
};

function seed(projects: ProjectSummary[] = [first, second]): void {
  useProjectStore.getState().reset();
  useProjectStore.setState({ projects, phase: "closed" });
}

function mockOpen() {
  const open = vi.fn(async (_folder: string, _options?: { discardCurrent?: boolean }) => true);
  useProjectStore.setState({ open });
  return open;
}

function mockResume() {
  const resume = vi.fn(async () => {});
  useProjectStore.setState({ resume });
  return resume;
}

beforeEach(() => {
  clearToasts();
  useProjectStore.getState().reset();
});

describe("welcome screen", () => {
  it("shows the brand, the three entry points and the recent projects", () => {
    seed();
    render(<WelcomeScreen backend={online} />);

    expect(screen.getByRole("heading", { name: "FuryBuilder" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New project" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open project" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Import world" })).toBeInTheDocument();
    expect(screen.getByText("Recent projects")).toBeInTheDocument();

    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("Second Base");
    expect(rows[0]).toHaveTextContent("Recoverable");
    expect(rows[1]).toHaveTextContent("First Castle");
  });

  it("opens the project manager on the requested creation mode", async () => {
    seed();
    const user = userEvent.setup();
    render(<WelcomeScreen backend={online} />);

    await user.click(screen.getByRole("button", { name: "New project" }));
    expect(useProjectStore.getState()).toMatchObject({
      managerOpen: true,
      managerTab: "create",
      managerCreateMode: "empty",
    });

    await user.click(screen.getByRole("button", { name: "Import world" }));
    expect(useProjectStore.getState()).toMatchObject({
      managerOpen: true,
      managerTab: "create",
      managerCreateMode: "import",
    });
  });

  it("explains that opening a folder needs the desktop core instead of opening a dialog", async () => {
    seed();
    const user = userEvent.setup();
    render(<WelcomeScreen backend={online} />);

    await user.click(screen.getByRole("button", { name: "Open project" }));

    expect(useProjectStore.getState().managerOpen).toBe(false);
    expect(getToasts().at(-1)?.title).toBe("Desktop core required");
  });

  it("opens a recent project by its folder", async () => {
    seed();
    const open = mockOpen();
    const user = userEvent.setup();
    render(<WelcomeScreen backend={online} />);

    await user.click(screen.getByRole("button", { name: "Open First Castle" }));
    expect(open).toHaveBeenCalledWith(first.folder);
  });

  it("restores a recoverable project from the recent list", async () => {
    seed();
    const open = mockOpen();
    const user = userEvent.setup();
    render(<WelcomeScreen backend={online} />);

    expect(screen.getByText("Unsaved recovery data")).toBeInTheDocument();
    expect(
      screen.getByText(/verified autosave is newer than the saved revision/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Recover Second Base" })).toHaveTextContent(
      "Recover",
    );
    expect(screen.queryByRole("button", { name: "Recover First Castle" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Recover Second Base" }));
    expect(open).toHaveBeenCalledWith(second.folder);
  });

  it("does not advertise recovery when every autosave is settled", () => {
    seed([{ ...first, recoverable: false }]);
    render(<WelcomeScreen backend={online} />);

    expect(screen.queryByText("Unsaved recovery data")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Recover / })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open First Castle" })).toBeInTheDocument();
  });

  it("shows an honest empty state without projects", () => {
    seed([]);
    render(<WelcomeScreen backend={online} />);

    expect(screen.getByText("No projects yet")).toBeInTheDocument();
    expect(screen.getByText(/Projects stay/)).toBeInTheDocument();
  });

  it("requires the desktop core in a browser preview", () => {
    seed();
    render(<WelcomeScreen backend={{ status: "offline", info: null, error: null }} />);

    expect(screen.getByText("Desktop core required")).toBeInTheDocument();
    expect(screen.getByText(/pnpm tauri dev/)).toBeInTheDocument();
  });

  it("replaces the project list with progress while preparing", () => {
    seed();
    useProjectStore.setState({ phase: "preparing", prepareMessage: "Restoring the last project…" });
    render(<WelcomeScreen backend={online} />);

    expect(screen.getByRole("status", { name: "Preparing project" })).toHaveTextContent(
      "Restoring the last project…",
    );
    expect(screen.queryByText("Recent projects")).not.toBeInTheDocument();
  });

  it("explains a lifecycle failure and retries it", async () => {
    seed();
    const resume = mockResume();
    useProjectStore.setState({
      phase: "error",
      failure: {
        code: "disk_space",
        message: "The volume is full.",
        hint: "Free disk space and try again.",
      },
      error: "The volume is full.",
    });
    const user = userEvent.setup();
    render(<WelcomeScreen backend={online} />);

    expect(screen.getByText("The volume is full.")).toBeInTheDocument();
    expect(screen.getByText("Free disk space and try again.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(resume).toHaveBeenCalledTimes(1);
  });

  it("warns about a project written by a newer schema", () => {
    seed([{ ...first, schemaVersion: 3 }]);
    render(<WelcomeScreen backend={online} />);

    expect(screen.getByText(/Newer project schema/)).toBeInTheDocument();
    expect(screen.getByText("Check project")).toBeInTheDocument();
  });
});
