import { useEffect, useRef, useState } from 'react';
import { ApplicationController } from './application';
import { errorMessages, nameSchema } from './domain';
import { Avatar } from './Avatar';
import { Icon } from './Icon';

export function CreateDialog({
  controller,
  creation,
  error,
}: {
  controller: ApplicationController;
  creation: ReturnType<ApplicationController['getState']>['creation'];
  error: ReturnType<ApplicationController['getState']>['error'];
}) {
  const pending = creation.kind === 'pending' ? creation : null;
  const [name, setName] = useState(pending?.draft.name ?? '');
  const [validation, setValidation] = useState<string | null>(null);
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => {
      dialog?.close();
    };
  }, []);
  return (
    <dialog
      ref={ref}
      className="create-dialog"
      aria-labelledby="create-title"
      onCancel={(event) => {
        event.preventDefault();
        controller.closeCreate();
      }}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          const parsed = nameSchema.safeParse(name);
          if (!parsed.success) {
            setValidation('Use 1 to 100 characters without control characters.');
            return;
          }
          setValidation(null);
          void controller.create(parsed.data);
        }}
      >
        <div className="dialog-heading">
          <Avatar />
          <button
            type="button"
            className="icon-button"
            aria-label="Close create bot"
            onClick={() => controller.closeCreate()}
          >
            <Icon name="close" />
          </button>
        </div>
        <h2 id="create-title">{pending ? 'Keep this bot identity.' : 'Meet your next bot.'}</h2>
        <p>
          {pending
            ? 'The request may have reached your manager. Retrying keeps the same name and identifier.'
            : 'Start with a name. It is saved in your local manager.'}
        </p>
        <label htmlFor="bot-name">Bot name</label>
        <input
          id="bot-name"
          autoFocus
          autoComplete="off"
          value={pending?.draft.name ?? name}
          readOnly={pending !== null}
          onChange={(event) => setName(event.target.value)}
          placeholder="For example, Research"
          required
        />
        {validation && (
          <p className="field-error" role="alert">
            {validation}
          </p>
        )}
        {error && (
          <p className="field-error" role="alert">
            {errorMessages[error]}
          </p>
        )}
        {pending && (
          <p className="creation-id">
            Saved identifier
            <br />
            <code>{pending.draft.id}</code>
          </p>
        )}
        <div className="dialog-actions">
          <button
            className="secondary-button"
            type="button"
            onClick={() => controller.closeCreate()}
          >
            {pending ? 'Keep for later' : 'Cancel'}
          </button>
          <button type="submit" className="primary-button" disabled={pending?.status === 'sending'}>
            {pending?.status === 'sending'
              ? 'Creating…'
              : pending
                ? 'Retry same bot'
                : 'Create bot'}
          </button>
        </div>
      </form>
    </dialog>
  );
}
