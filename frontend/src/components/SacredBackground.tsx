import NeuralNetworkBackground from './NeuralNetworkBackground';

export default function SacredBackground() {
  return (
    <div className="fixed inset-0 z-0" aria-hidden="true" style={{ pointerEvents: 'none' }}>
      {/* Deep void base — full page */}
      <div className="fixed inset-0" style={{ background: '#02020a' }} />

      {/* Ambient energy glow — 青蓝主 + 紫罗兰辅（透明度降低，避免形成膜感）*/}
      <div className="fixed w-[600px] h-[600px] rounded-full"
        style={{ top: '5%', left: '50%', transform: 'translateX(-50%)',
          background: 'radial-gradient(circle, rgba(62, 207, 174, 0.05) 0%, transparent 55%)' }} />
      <div className="fixed w-[450px] h-[450px] rounded-full"
        style={{ bottom: '5%', right: '0%',
          background: 'radial-gradient(circle, rgba(139, 126, 200, 0.04) 0%, transparent 50%)' }} />
      <div className="fixed w-[400px] h-[400px] rounded-full"
        style={{ top: '40%', left: '0%',
          background: 'radial-gradient(circle, rgba(139, 126, 200, 0.035) 0%, transparent 50%)' }} />

      {/* Neural Network — fixed to viewport, covers full scroll range */}
      <NeuralNetworkBackground />

      {/* ── 全息扫描线 ── 周期性青蓝光束从上往下扫 */}
      <div className="holo-scan-line" />

      {/* 极轻微暗角 — 仅在最边缘聚焦，不形成光膜 */}
      <div className="fixed inset-0"
        style={{ background: 'radial-gradient(ellipse at center, transparent 70%, rgba(2,2,10,0.15) 100%)' }} />
    </div>
  );
}
