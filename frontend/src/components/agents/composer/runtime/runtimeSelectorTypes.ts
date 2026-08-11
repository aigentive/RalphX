import type { ReactNode } from "react";

import type { CapabilityIntent } from "@/api/chat";
import type { AgentProvider } from "@/stores/agentSessionStore";

export interface ComposerRuntimeOption {
  id: string;
  label: string;
  description?: string;
  disabled?: boolean;
  disabledReason?: string;
}

export interface ComposerRuntimeProviderField {
  value: AgentProvider;
  onValueChange: (value: AgentProvider) => void;
  options: Array<ComposerRuntimeOption & { id: AgentProvider }>;
  disabled?: boolean;
  footerAction?: ReactNode;
  compactFooterAction?: ReactNode;
  testId?: string;
  className?: string;
}

export interface ComposerRuntimeModelField {
  value: string;
  onValueChange: (value: string) => void;
  options: ComposerRuntimeOption[];
  disabled?: boolean;
  allowCustomValue?: boolean;
  customPlaceholder?: string | undefined;
  onOpenModelSettings?: () => void;
  fastMode?: {
    visible: boolean;
    value: boolean;
    onValueChange: (value: boolean) => void;
    disabled?: boolean;
    description?: string;
    testId?: string;
  };
  testId?: string;
  className?: string;
}

export interface ComposerRuntimeEffortField {
  value: string;
  onValueChange: (value: string) => void;
  options: ComposerRuntimeOption[];
  disabled?: boolean;
  testId?: string;
  className?: string;
}

export interface ComposerRuntimeCapabilityField {
  value: CapabilityIntent["coordinationMode"];
  onValueChange: (
    value: CapabilityIntent["coordinationMode"],
  ) => void | Promise<unknown>;
  options: ComposerRuntimeOption[];
  disabled?: boolean;
  pending?: boolean;
  testId?: string;
}

export interface ComposerRuntimePersonaField {
  value: string;
  onValueChange: (value: string) => void | Promise<unknown>;
  options: ComposerRuntimeOption[];
  disabled?: boolean;
  testId?: string;
  footerAction?: ReactNode;
}

export interface ComposerRuntimeSpeedField {
  value: string;
  onValueChange: (value: string) => void;
  options: ComposerRuntimeOption[];
  disabled?: boolean;
  testId?: string;
}
