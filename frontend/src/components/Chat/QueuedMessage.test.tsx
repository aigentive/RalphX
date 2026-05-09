/**
 * Tests for QueuedMessage component
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueuedMessage } from "./QueuedMessage";
import type { QueuedMessage as QueuedMessageType } from "@/stores/chatStore";

describe("QueuedMessage", () => {
  const createMockMessage = (overrides?: Partial<QueuedMessageType>): QueuedMessageType => ({
    id: "test-message-1",
    content: "This is a test message",
    createdAt: new Date().toISOString(),
    isEditing: false,
    ...overrides,
  });

  it("renders the message content", () => {
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    expect(screen.getByTestId("queued-message-content")).toHaveTextContent(
      "This is a test message"
    );
  });

  it("displays send icon indicator", () => {
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    const messageElement = screen.getByTestId("queued-message");
    expect(messageElement).toBeInTheDocument();
  });

  it("shows edit and delete buttons when not editing", () => {
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    expect(screen.getByTestId("queued-message-edit")).toBeInTheDocument();
    expect(screen.getByTestId("queued-message-delete")).toBeInTheDocument();
  });

  it("calls onDelete when delete button is clicked", () => {
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    fireEvent.click(screen.getByTestId("queued-message-delete"));

    expect(onDelete).toHaveBeenCalledWith("test-message-1");
  });

  it("enters edit mode when edit button is clicked", async () => {
    const user = userEvent.setup();
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    await user.click(screen.getByTestId("queued-message-edit"));

    expect(screen.getByTestId("queued-message-edit-input")).toBeInTheDocument();
    expect(screen.getByTestId("queued-message-edit-input")).toHaveValue(
      "This is a test message"
    );
  });

  it("shows save and cancel buttons in edit mode", async () => {
    const user = userEvent.setup();
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    await user.click(screen.getByTestId("queued-message-edit"));

    expect(screen.getByTestId("queued-message-save")).toBeInTheDocument();
    expect(screen.getByTestId("queued-message-cancel")).toBeInTheDocument();
  });

  it("calls onEdit with new content when save is clicked", async () => {
    const user = userEvent.setup();
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    await user.click(screen.getByTestId("queued-message-edit"));

    const input = screen.getByTestId("queued-message-edit-input");
    await user.clear(input);
    await user.type(input, "Updated message");

    await user.click(screen.getByTestId("queued-message-save"));

    expect(onEdit).toHaveBeenCalledWith("test-message-1", "Updated message");
  });

  it("cancels edit mode when cancel button is clicked", async () => {
    const user = userEvent.setup();
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    await user.click(screen.getByTestId("queued-message-edit"));

    const input = screen.getByTestId("queued-message-edit-input");
    await user.clear(input);
    await user.type(input, "Modified content");

    await user.click(screen.getByTestId("queued-message-cancel"));

    // Should exit edit mode and restore original content
    await waitFor(() => {
      expect(screen.queryByTestId("queued-message-edit-input")).not.toBeInTheDocument();
    });
    expect(screen.getByTestId("queued-message-content")).toHaveTextContent(
      "This is a test message"
    );
    expect(onEdit).not.toHaveBeenCalled();
  });

  it("saves edit when Enter is pressed", async () => {
    const user = userEvent.setup();
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    await user.click(screen.getByTestId("queued-message-edit"));

    const input = screen.getByTestId("queued-message-edit-input");
    await user.clear(input);
    await user.type(input, "Enter saves this");
    await user.keyboard("{Enter}");

    expect(onEdit).toHaveBeenCalledWith("test-message-1", "Enter saves this");
  });

  it("cancels edit when Escape is pressed", async () => {
    const user = userEvent.setup();
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    await user.click(screen.getByTestId("queued-message-edit"));

    const input = screen.getByTestId("queued-message-edit-input");
    await user.clear(input);
    await user.type(input, "Escape cancels");
    await user.keyboard("{Escape}");

    await waitFor(() => {
      expect(screen.queryByTestId("queued-message-edit-input")).not.toBeInTheDocument();
    });
    expect(onEdit).not.toHaveBeenCalled();
  });

  it("allows Shift+Enter for newline in edit mode", async () => {
    const user = userEvent.setup();
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    await user.click(screen.getByTestId("queued-message-edit"));

    const input = screen.getByTestId("queued-message-edit-input");
    await user.clear(input);
    await user.type(input, "Line 1{Shift>}{Enter}{/Shift}Line 2");

    // Should still be in edit mode
    expect(screen.getByTestId("queued-message-edit-input")).toBeInTheDocument();
    expect(onEdit).not.toHaveBeenCalled();
  });

  it("disables save button when content is empty", async () => {
    const user = userEvent.setup();
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    await user.click(screen.getByTestId("queued-message-edit"));

    const input = screen.getByTestId("queued-message-edit-input");
    await user.clear(input);

    const saveButton = screen.getByTestId("queued-message-save");
    expect(saveButton).toBeDisabled();
  });

  it("disables save button when content is only whitespace", async () => {
    const user = userEvent.setup();
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    await user.click(screen.getByTestId("queued-message-edit"));

    const input = screen.getByTestId("queued-message-edit-input");
    await user.clear(input);
    await user.type(input, "   ");

    const saveButton = screen.getByTestId("queued-message-save");
    expect(saveButton).toBeDisabled();
  });

  it("starts in edit mode if message.isEditing is true", () => {
    const message = createMockMessage({ isEditing: true });
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    expect(screen.getByTestId("queued-message-edit-input")).toBeInTheDocument();
  });

  it("enters edit mode with the full message when isEditing changes after render", () => {
    const longContent = `Start ${"A".repeat(500)} End`;
    const message = createMockMessage({ content: longContent });
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    const { rerender } = render(
      <QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />
    );

    rerender(
      <QueuedMessage
        message={{ ...message, isEditing: true }}
        onEdit={onEdit}
        onDelete={onDelete}
      />
    );

    expect(screen.getByTestId("queued-message-edit-input")).toHaveValue(longContent);
  });

  it("trims whitespace when saving", async () => {
    const user = userEvent.setup();
    const message = createMockMessage();
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    await user.click(screen.getByTestId("queued-message-edit"));

    const input = screen.getByTestId("queued-message-edit-input");
    await user.clear(input);
    await user.type(input, "  Trimmed content  ");

    await user.click(screen.getByTestId("queued-message-save"));

    expect(onEdit).toHaveBeenCalledWith("test-message-1", "Trimmed content");
  });

  it("renders long messages as an excerpt in read-only mode", () => {
    const longContent = `Start ${"A".repeat(500)} End`;
    const message = createMockMessage({ content: longContent });
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    const preview = screen.getByTestId("queued-message-content");
    expect(preview).not.toHaveTextContent(longContent);
    expect(preview.textContent).toMatch(/^Start A+/);
    expect(preview.textContent).toMatch(/\.\.\.$/);
  });

  it("keeps the full message available when editing an excerpted queued message", async () => {
    const user = userEvent.setup();
    const longContent = `Start ${"A".repeat(500)} End`;
    const message = createMockMessage({ content: longContent });
    const onEdit = vi.fn();
    const onDelete = vi.fn();

    render(<QueuedMessage message={message} onEdit={onEdit} onDelete={onDelete} />);

    await user.click(screen.getByTestId("queued-message-edit"));

    expect(screen.getByTestId("queued-message-edit-input")).toHaveValue(longContent);
  });
});
