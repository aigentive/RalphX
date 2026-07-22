import { FileText, Lightbulb } from 'lucide-react';
import { Button } from '@/components/ui/button';

interface EmptySearchStateProps {
  searchQuery: string;
  onCreateTask?: () => void;
  onClearSearch: () => void;
  showArchived: boolean;
}

export function EmptySearchState({
  searchQuery,
  onCreateTask,
  onClearSearch,
  showArchived,
}: EmptySearchStateProps) {
  return (
    <div className="flex flex-col items-center justify-center py-16 text-center">
      {/* Icon */}
      <FileText className="w-12 h-12 text-muted-foreground mb-4" />

      {/* Heading */}
      <h3 className="text-lg font-medium mb-2">
        No tasks match "{searchQuery}"
      </h3>

      {/* Subheading */}
      <p className="text-muted-foreground mb-6">Should this be a task?</p>

      {/* Buttons */}
      <div className="flex gap-3">
        {onCreateTask && (
          <Button onClick={onCreateTask} variant="default">
            + Create "{searchQuery}"
          </Button>
        )}
        <Button onClick={onClearSearch} variant="outline">
          Clear Search
        </Button>
      </div>

      {/* Tip - only shown if archived is not shown */}
      {!showArchived && (
        <div className="mt-6 p-3 bg-muted/50 rounded-lg flex items-start gap-2 max-w-md">
          <Lightbulb className="w-5 h-5 text-muted-foreground flex-shrink-0 mt-0.5" />
          <p className="text-sm text-muted-foreground text-left">
            Tip: Enable "Show archived" to search old tasks
          </p>
        </div>
      )}
    </div>
  );
}
