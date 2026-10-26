export function Avatar({
  seed = 'Botstral',
  large = false,
  primary = false,
}: {
  seed?: string;
  large?: boolean;
  primary?: boolean;
}) {
  const variant = primary
    ? 0
    : 1 + (Array.from(seed).reduce((sum, char) => sum + (char.codePointAt(0) ?? 0), 0) % 3);
  const shapes = [
    'M10 23C16 5 37 3 48 15c13 13 10 34-6 41C25 64 6 51 7 36c0-5 1-9 3-13Z',
    'M15 7c6-6 14 3 23 9 12 8 24 13 19 22-4 8-19 12-29 17C11 64 7 55 9 43l3-22c1-6 0-11 3-14Z',
    'M10 17c5-11 16-8 23-4C45 4 56 15 53 26c12 12 4 28-11 27-13 12-29 5-27-8C2 39 1 26 10 17Z',
    'M19 8c9-6 22 2 24 11 17 2 19 17 7 25-1 16-19 19-27 9C5 55 1 40 11 30 5 19 9 11 19 8Z',
  ];
  return (
    <span className={`avatar face-${variant}${large ? ' avatar-large' : ''}`} aria-hidden="true">
      <svg viewBox={variant === 0 ? '6 5 53 55' : '4 4 56 56'} fill="none">
        <path className="face-shape" d={shapes[variant]} />
        <path className="face-eyes" d="m31 18 4 6m7-10 4 6" />
      </svg>
    </span>
  );
}
