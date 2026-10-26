type Name =
  | 'plus'
  | 'search'
  | 'panel'
  | 'refresh'
  | 'close'
  | 'sun'
  | 'moon'
  | 'chevron'
  | 'bell'
  | 'mic'
  | 'waveform'
  | 'logout';
const paths: Record<Name, string> = {
  plus: 'M12 5v14M5 12h14',
  search: 'm20 20-4-4M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13Z',
  panel: 'M15 4v16M5 4h14a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1Z',
  refresh: 'M20 7v5h-5M4 17v-5h5M6 7a7 7 0 0 1 12-1l2 6M4 12l2 6a7 7 0 0 0 12-1',
  close: 'm6 6 12 12M18 6 6 18',
  sun: 'M12 3v2m0 14v2M3 12h2m14 0h2M5.5 5.5 7 7m10 10 1.5 1.5M5.5 18.5 7 17M17 7l1.5-1.5M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z',
  moon: 'M20 14a8 8 0 0 1-10-10 8 8 0 1 0 10 10Z',
  chevron: 'm9 5 7 7-7 7',
  bell: 'M6 10a6 6 0 0 1 12 0v5l2 2H4l2-2v-5Zm4 10h4',
  mic: 'M9 6a3 3 0 0 1 6 0v6a3 3 0 0 1-6 0V6Zm-3 6a6 6 0 0 0 12 0M12 18v3m-3 0h6',
  waveform: 'M4 10v4m4-7v10m4-13v16m4-13v10m4-7v4',
  logout: 'M10 4H5v16h5m5-12 4 4-4 4M9 12h10',
};
export function Icon({ name, size = 18 }: { name: Name; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[name]} />
    </svg>
  );
}
