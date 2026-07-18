---
name: testing-patterns
description: TDD workflow and testing patterns
disable-model-invocation: true
user-invocable: false
---

# Testing Patterns

## TDD Workflow

### The Cycle
1. **Write a failing test** - Define expected behavior
2. **Implement** - Minimum code to pass
3. **Refactor** - Improve code quality
4. **Repeat** - Next test case

### Why TDD
- Tests define requirements clearly
- Catches regressions early
- Forces modular design
- Documents expected behavior

## TypeScript Testing (Vitest)

### Setup
```typescript
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
```

### Component Tests
```typescript
describe('MyComponent', () => {
  it('should render the title', () => {
    render(<MyComponent title="Hello" />);
    expect(screen.getByText('Hello')).toBeInTheDocument();
  });

  it('should call onChange when clicked', async () => {
    const onChange = vi.fn();
    render(<MyComponent onChange={onChange} />);

    await userEvent.click(screen.getByRole('button'));
    expect(onChange).toHaveBeenCalled();
  });
});
```

### Hook Tests
```typescript
import { renderHook, act } from '@testing-library/react';

it('should increment counter', () => {
  const { result } = renderHook(() => useCounter());

  act(() => {
    result.current.increment();
  });

  expect(result.current.count).toBe(1);
});
```

### Mocking
```typescript
// Mock Tauri invoke
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

// Mock module
vi.mock('./api', () => ({
  fetchData: vi.fn().mockResolvedValue({ data: [] }),
}));
```

## Rust Testing

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_addition() {
        assert_eq!(add(2, 2), 4);
    }

    #[test]
    #[should_panic(expected = "divide by zero")]
    fn test_divide_by_zero() {
        divide(1, 0);
    }
}
```

### Async Tests
```rust
#[tokio::test]
async fn test_async_operation() {
    let result = fetch_data().await;
    assert!(result.is_ok());
}
```

### Integration Tests
```rust
// tests/integration.rs
use ralphx_lib::AppState;

#[tokio::test]
async fn test_full_workflow() {
    let state = AppState::new_test();
    // Test complete workflow
}
```

## Commands

Follow the target project's local validation instructions. Select only the affected test file, module, crate, or named test; do not turn these examples into a full-suite completion gate.

```bash
# TypeScript
npm run test:run -- <affected-test-file>

# Rust
cargo test <test_or_module_filter> --lib
cargo nextest run --test <affected-suite> -E 'test(<test_or_module_filter>)'
```
