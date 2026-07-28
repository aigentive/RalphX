/**
 * The 2.6-a matrix: host-impossible affordances, keyed on environment kind ONLY.
 *
 * Every case runs both columns. The LOCAL column is the non-regression half — it is
 * what proves the gating code is inert when the flag is off / the environment is
 * local, which is the whole dark-ship promise. The REMOTE column's load-bearing
 * assertions are the ABSENCE ones: `openPath` was not called, `convertFileSrc` was
 * not called, the control is not in the document. A test that only checked "a hint
 * appeared" would pass against a UI that still fires the host-only side effect.
 */

import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  HOST_ATTACHMENT_HINT,
  HOST_ONLY_AFFORDANCE_HINT,
} from "@/lib/remote/host-affordances";

const openPathMock = vi.fn(async () => {});
const revealItemInDirMock = vi.fn(async () => {});
const convertFileSrcMock = vi.fn((path: string) => `asset://${path}`);

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: (path: string) => openPathMock(path),
  revealItemInDir: (path: string) => revealItemInDirMock(path),
}));

vi.mock("@tauri-apps/api/core", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return {
    ...actual,
    convertFileSrc: (path: string) => convertFileSrcMock(path),
  };
});

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const REMOTE_ID = "env-remote";

function setEnvironment(kind: "local" | "remote"): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: kind === "local" ? LOCAL_ENVIRONMENT_ID : REMOTE_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: REMOTE_ID, name: "Studio Mac", kind: "remote" },
    ],
  });
}

function withTooltips(node: React.ReactNode) {
  return <TooltipProvider>{node}</TooltipProvider>;
}

beforeEach(() => {
  vi.clearAllMocks();
  setEnvironment("local");
});

// ---------------------------------------------------------------------------
// Chat file links — openPath / revealItemInDir must never fire for a host path
// ---------------------------------------------------------------------------

describe("chat markdown file links", () => {
  async function renderLink() {
    const { MarkdownLink } =
      await import("@/components/Chat/MessageItem.markdown");
    return render(
      withTooltips(
        <MarkdownLink href="file:///Users/host/project/src/main.rs">
          main.rs
        </MarkdownLink>,
      ),
    );
  }

  it("local: keeps the clickable local-file link and opens it", async () => {
    await renderLink();
    const link = screen.getByText("main.rs");
    expect(screen.queryByTestId("chat-remote-host-file-link")).toBeNull();

    link.click();
    expect(openPathMock).toHaveBeenCalledWith(
      "/Users/host/project/src/main.rs",
    );
  });

  it("remote: renders a copy-path affordance and never opens the path", async () => {
    setEnvironment("remote");
    await renderLink();

    expect(
      screen.getByTestId("chat-remote-host-file-link"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("chat-remote-host-file-copy")).toHaveAttribute(
      "aria-label",
    );

    screen.getByTestId("chat-remote-host-file-link").click();
    screen.getByText("main.rs").click();

    expect(openPathMock).not.toHaveBeenCalled();
    expect(revealItemInDirMock).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Chat attachments — convertFileSrc mints an asset:// URL for THIS device only
// ---------------------------------------------------------------------------

describe("chat attachments", () => {
  const imageAttachment = {
    id: "att-1",
    fileName: "screenshot.png",
    fileSize: 2048,
    mimeType: "image/png",
    filePath: "/Users/host/Desktop/screenshot.png",
  };

  async function renderAttachments() {
    const { MessageAttachments } =
      await import("@/components/Chat/MessageAttachments");
    return render(
      withTooltips(
        <MessageAttachments
          attachments={[imageAttachment as never]}
          onClick={vi.fn()}
        />,
      ),
    );
  }

  it("local: renders the image preview through convertFileSrc", async () => {
    await renderAttachments();
    expect(convertFileSrcMock).toHaveBeenCalledWith(
      "/Users/host/Desktop/screenshot.png",
    );
    expect(screen.getByTestId("attachment-image-preview")).toBeInTheDocument();
    expect(screen.queryByTestId("attachment-host-card")).toBeNull();
  });

  it("remote: renders the on-host placeholder and never calls convertFileSrc", async () => {
    setEnvironment("remote");
    await renderAttachments();

    expect(convertFileSrcMock).not.toHaveBeenCalled();
    expect(screen.queryByTestId("attachment-image-preview")).toBeNull();
    expect(screen.getByTestId("attachment-host-card")).toBeInTheDocument();
    expect(screen.getByTestId("attachment-host-hint")).toHaveTextContent(
      HOST_ATTACHMENT_HINT,
    );
    expect(screen.getByText("screenshot.png")).toBeInTheDocument();
    expect(screen.getByTestId("attachment-chip")).toBeDisabled();
    expect(screen.getByTestId("attachment-host-copy")).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------------------
// Workspace Open menu — editor / file-manager / built-in terminal
// ---------------------------------------------------------------------------

describe("workspace open control", () => {
  const targets = [
    { id: "vscode", label: "VS Code", kind: "editor" as const },
    { id: "finder", label: "Finder", kind: "fileManager" as const },
  ];

  async function renderControl() {
    const { AgentsWorkspaceOpenControl } =
      await import("@/components/agents/AgentsWorkspaceOpenControl");
    const onOpenTarget = vi.fn();
    render(
      withTooltips(
        <AgentsWorkspaceOpenControl
          targets={targets as never}
          onOpenTarget={onOpenTarget}
          openingTargetId={null}
          builtInTerminal={{
            open: false,
            onToggle: vi.fn(),
            unavailableReason: null,
          }}
        />,
      ),
    );
    return onOpenTarget;
  }

  it("local: the primary open control is enabled and dispatches", async () => {
    const onOpenTarget = await renderControl();
    const primary = screen.getByTestId("agents-open-workspace");
    expect(primary).toBeEnabled();

    primary.click();
    expect(onOpenTarget).toHaveBeenCalled();
  });

  it("remote: the open control is disabled, explained, and does not dispatch", async () => {
    setEnvironment("remote");
    const onOpenTarget = await renderControl();

    const primary = screen.getByTestId("agents-open-workspace");
    expect(primary).toBeDisabled();
    expect(primary.getAttribute("aria-label")).toContain(
      HOST_ONLY_AFFORDANCE_HINT,
    );

    primary.click();
    expect(onOpenTarget).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Welcome screen — project creation is host-only
// ---------------------------------------------------------------------------

describe("welcome screen project creation", () => {
  async function renderWelcome() {
    const WelcomeScreen = (
      await import("@/components/WelcomeScreen/WelcomeScreen")
    ).default;
    const onCreateProject = vi.fn();
    render(
      withTooltips(
        <WelcomeScreen onCreateProject={onCreateProject} hasProjects={false} />,
      ),
    );
    return onCreateProject;
  }

  it("local: offers the create-project CTA", async () => {
    await renderWelcome();
    expect(
      screen.getByTestId("create-first-project-button"),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("welcome-remote-no-create")).toBeNull();
  });

  it("remote: hides the CTA and explains where projects are created", async () => {
    setEnvironment("remote");
    const onCreateProject = await renderWelcome();

    expect(screen.queryByTestId("create-first-project-button")).toBeNull();
    expect(screen.getByTestId("welcome-remote-no-create")).toBeInTheDocument();

    // The ⌘N shortcut is part of the same affordance and must be gated with it.
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "n", metaKey: true, bubbles: true }),
    );
    expect(onCreateProject).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// Hide-site guard for the surfaces whose hosts are too heavy to mount here
// ---------------------------------------------------------------------------

/**
 * `App.tsx`, `AgentsChatHeader`, `ExecutionControlBar` and `AgentsSidebar` each hide
 * a host-only affordance behind the env-kind hook. Mounting them means standing up
 * the whole app tree, so the guard is structural instead: it asserts the gate is
 * still WIRED at each site. It cannot prove the rendered outcome — that is what the
 * behavioural cases above do for the surfaces that can be mounted — but it does
 * catch the realistic regression, which is a later refactor dropping the condition.
 */
describe("host-only hide sites stay gated", () => {
  const cases: ReadonlyArray<{
    readonly file: string;
    readonly needles: readonly string[];
  }> = [
    {
      file: "src/App.tsx",
      needles: [
        "useIsRemoteEnvironment",
        "const canCreateProjects = !isRemoteEnvironment",
        "{canCreateProjects && (",
        "canCreateProjects ? { onNewProject: handleOpenProjectWizard } : {}",
      ],
    },
    {
      file: "src/components/agents/AgentsChatHeader.tsx",
      needles: ["useIsRemoteEnvironment", "{!isRemoteEnvironment && ("],
    },
    {
      file: "src/components/execution/ExecutionControlBar.tsx",
      needles: [
        "useIsRemoteEnvironment",
        "terminalCount > 0 && !isRemoteEnvironment",
      ],
    },
    {
      file: "src/components/agents/AgentsSidebar.tsx",
      needles: ["useIsRemoteEnvironment", "{!isRemoteEnvironment && ("],
    },
  ];

  it.each(cases)("$file keeps its env-kind gate", async ({ file, needles }) => {
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    const source = readFileSync(resolve(__dirname, "../../..", file), "utf8");
    for (const needle of needles) {
      expect(source).toContain(needle);
    }
  });
});
