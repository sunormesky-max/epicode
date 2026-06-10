import NeuralNetworkBackground from './NeuralNetworkBackground';

export default function SacredBackground() {
  return (
    <div className="fixed inset-0 z-0" aria-hidden="true" style={{ pointerEvents: 'none' }}>
      {/* Deep void base — full page */}
      <div className="fixed inset-0" style={{ background: '#030305' }} />

      {/* Subtle ambient glow */}
      <div className="fixed w-[500px] h-[500px] rounded-full"
        style={{ top: '5%', left: '50%', transform: 'translateX(-50%)',
          background: 'radial-gradient(circle, rgba(168, 85, 247, 0.07) 0%, transparent 55%)' }} />
      <div className="fixed w-[400px] h-[400px] rounded-full"
        style={{ bottom: '10%', right: '0%',
          background: 'radial-gradient(circle, rgba(217, 70, 239, 0.05) 0%, transparent 50%)' }} />

      {/* Neural Network — fixed to viewport, covers full scroll range */}
      <NeuralNetworkBackground />

      {/* Subtle vignette for depth */}
      <div className="fixed inset-0"
        style={{ background: 'radial-gradient(ellipse at center, transparent 40%, rgba(3,3,5,0.5) 100%)' }} />
    </div>
  );
}
