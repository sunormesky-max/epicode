import SacredBackground from './SacredBackground';

// ── PageBackground ──
// All pages use the SAME neural network background as the home page.
// This ensures visual consistency across the entire app.

export default function PageBackground() {
  return (
    <div className="fixed inset-0 z-0" style={{ background: '#030305' }}>
      <SacredBackground />
    </div>
  );
}
