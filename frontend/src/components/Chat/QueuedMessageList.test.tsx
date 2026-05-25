/**
 * Tests for QueuedMessageList component
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueuedMessageList } from "./QueuedMessageList";
import type { QueuedMessage as QueuedMessageType } from "@/stores/chatStore";

describe("QueuedMessageList", () => {
  const createMockMessage = (
    id: string,
    content: string,
    overrides?: Partial<QueuedMessageType>
  ): QueuedMessageType => ({
    id,
    content,
    createdAt: new Date().toISOString(),
    isEditing: false,
    attachmentIds: [],
    ...overrides,
  });

  it("does not render when messages array is empty", () => {
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessageList messages={[]} onEdit={onEdit} onDelete={onDelete} />);

    expect(screen.queryByTestId("queued-message-list")).not.toBeInTheDocument();
  });

  it("renders header with message count", () => {
    const messages = [
      createMockMessage("msg-1", "Message 1"),
      createMockMessage("msg-2", "Message 2"),
      createMockMessage("msg-3", "Message 3"),
    ];
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessageList messages={messages} onEdit={onEdit} onDelete={onDelete} />);

    expect(screen.getByText("Queued Messages (3)")).toBeInTheDocument();
  });

  it("renders explanatory text", () => {
    const messages = [createMockMessage("msg-1", "Message 1")];
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessageList messages={messages} onEdit={onEdit} onDelete={onDelete} />);

    expect(
      screen.getByText("These messages will be sent when the agent finishes.")
    ).toBeInTheDocument();
  });

  it("renders all queued messages", () => {
    const messages = [
      createMockMessage("msg-1", "First message"),
      createMockMessage("msg-2", "Second message"),
      createMockMessage("msg-3", "Third message"),
    ];
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessageList messages={messages} onEdit={onEdit} onDelete={onDelete} />);

    expect(screen.getByText("First message")).toBeInTheDocument();
    expect(screen.getByText("Second message")).toBeInTheDocument();
    expect(screen.getByText("Third message")).toBeInTheDocument();
  });

  it("passes onEdit callback to QueuedMessage components", async () => {
    const user = userEvent.setup();
    const messages = [createMockMessage("msg-1", "Test message")];
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessageList messages={messages} onEdit={onEdit} onDelete={onDelete} />);

    // Click edit button
    const editButton = screen.getByTestId("queued-message-edit");
    await user.click(editButton);

    // Edit the message
    const input = screen.getByTestId("queued-message-edit-input");
    await user.clear(input);
    await user.type(input, "Edited message");

    // Save the edit
    await user.click(screen.getByTestId("queued-message-save"));

    expect(onEdit).toHaveBeenCalledWith("msg-1", "Edited message", []);
  });

  it("passes onDelete callback to QueuedMessage components", async () => {
    const user = userEvent.setup();
    const messages = [createMockMessage("msg-1", "Test message")];
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessageList messages={messages} onEdit={onEdit} onDelete={onDelete} />);

    // Click delete button
    const deleteButton = screen.getByTestId("queued-message-delete");
    await user.click(deleteButton);

    expect(onDelete).toHaveBeenCalledWith("msg-1");
  });

  it("passes onSendNow callback to QueuedMessage components", async () => {
    const user = userEvent.setup();
    const messages = [createMockMessage("msg-1", "Message 1")];
    const onEdit = vi.fn();
    const onDelete = vi.fn();
    const onSendNow = vi.fn();

    render(
      <QueuedMessageList
        messages={messages}
        onEdit={onEdit}
        onDelete={onDelete}
        onSendNow={onSendNow}
      />
    );

    await user.click(screen.getByTestId("queued-message-send-now"));

    expect(onSendNow).toHaveBeenCalledWith("msg-1", "Message 1", []);
  });

  it("renders messages in correct order", () => {
    const messages = [
      createMockMessage("msg-1", "First"),
      createMockMessage("msg-2", "Second"),
      createMockMessage("msg-3", "Third"),
    ];
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessageList messages={messages} onEdit={onEdit} onDelete={onDelete} />);

    const messageElements = screen.getAllByTestId("queued-message");
    expect(messageElements).toHaveLength(3);
    expect(messageElements[0]).toHaveAttribute("data-message-id", "msg-1");
    expect(messageElements[1]).toHaveAttribute("data-message-id", "msg-2");
    expect(messageElements[2]).toHaveAttribute("data-message-id", "msg-3");
  });

  it("updates count when single message is present", () => {
    const messages = [createMockMessage("msg-1", "Only message")];
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessageList messages={messages} onEdit={onEdit} onDelete={onDelete} />);

    expect(screen.getByText("Queued Messages (1)")).toBeInTheDocument();
  });

  it("handles large number of messages", () => {
    const messages = Array.from({ length: 20 }, (_, i) =>
      createMockMessage(`msg-${i}`, `Message ${i + 1}`)
    );
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessageList messages={messages} onEdit={onEdit} onDelete={onDelete} />);

    expect(screen.getByText("Queued Messages (20)")).toBeInTheDocument();
    expect(screen.getAllByTestId("queued-message")).toHaveLength(20);
  });

  it("applies correct styling to container", () => {
    const messages = [createMockMessage("msg-1", "Test message")];
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessageList messages={messages} onEdit={onEdit} onDelete={onDelete} />);

    const container = screen.getByTestId("queued-message-list");
    expect(container).toHaveClass("rounded-lg", "p-4", "mb-4");
  });

  it("renders with messages that have different properties", () => {
    const messages = [
      createMockMessage("msg-1", "Short"),
      createMockMessage("msg-2", "A".repeat(200)),
      createMockMessage("msg-3", "Message with\nmultiple\nlines"),
    ];
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessageList messages={messages} onEdit={onEdit} onDelete={onDelete} />);

    expect(screen.getByText("Short")).toBeInTheDocument();
    expect(screen.getByText("A".repeat(200))).toBeInTheDocument();
    // Use a function matcher for multiline text to be more flexible
    expect(
      screen.getByText((content, element) => {
        return (
          element?.tagName.toLowerCase() === "p" &&
          content.includes("Message with") &&
          content.includes("multiple") &&
          content.includes("lines")
        );
      })
    ).toBeInTheDocument();
  });
});
