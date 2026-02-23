"use client";

import { motion } from "framer-motion";
import Link from "next/link";
import {
  Cloud,
  Check,
  MoreHorizontal,
  MousePointer2,
  Crop,
  Type,
  Square,
  Video,
  ChevronRight,
  Sparkles,
  Command,
  Settings2
} from "lucide-react";

export function Hero() {
  return (
    <section className="relative min-h-screen bg-[#000000] overflow-hidden">
      {/* Premium Ambient Background */}
      <div className="absolute inset-0 pointer-events-none flex items-center justify-center overflow-hidden bg-[#000000]">

        {/* Massive Soft Ambient Orbs */}
        <motion.div
          className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] sm:w-[800px] sm:h-[800px] rounded-full mix-blend-screen opacity-50 blur-[100px] sm:blur-[140px]"
          style={{
            background: 'conic-gradient(from 180deg at 50% 50%, rgba(24,24,27,0) 0deg, rgba(255,255,255,0.15) 120deg, rgba(24,24,27,0) 240deg, rgba(24,24,27,0) 360deg)'
          }}
          animate={{
            rotate: [0, 360],
          }}
          transition={{ duration: 40, repeat: Infinity, ease: "linear" }}
        />

        <motion.div
          className="absolute top-0 right-[10%] w-[400px] h-[400px] rounded-full mix-blend-screen opacity-[0.8] blur-[100px]"
          style={{
            background: 'radial-gradient(circle, rgba(255,255,255,0.1) 0%, transparent 70%)'
          }}
          animate={{
            x: [0, -50, 0],
            y: [0, 50, 0],
          }}
          transition={{ duration: 15, repeat: Infinity, ease: "easeInOut" }}
        />

        <motion.div
          className="absolute bottom-0 left-[10%] w-[500px] h-[500px] rounded-full mix-blend-screen opacity-[0.6] blur-[120px]"
          style={{
            background: 'radial-gradient(circle, rgba(255,255,255,0.08) 0%, transparent 70%)'
          }}
          animate={{
            x: [0, 60, 0],
            y: [0, -30, 0],
          }}
          transition={{ duration: 20, repeat: Infinity, ease: "easeInOut" }}
        />

        {/* Fine, delicate noise grain for texture (very subtle) */}
        <div
          className="absolute inset-0 z-20 opacity-[0.25] mix-blend-overlay pointer-events-none"
          style={{
            backgroundImage: "url('data:image/svg+xml,%3Csvg viewBox=\"0 0 200 200\" xmlns=\"http://www.w3.org/2000/svg\"%3E%3Cfilter id=\"noiseFilter\"%3E%3CfeTurbulence type=\"fractalNoise\" baseFrequency=\"1.5\" numOctaves=\"3\" stitchTiles=\"stitch\"/%3E%3C/filter%3E%3Crect width=\"100%25\" height=\"100%25\" filter=\"url(%23noiseFilter)\"/%3E%3C/svg%3E')"
          }}
        />

        {/* Dark Vignette Overlay to maintain focus and extreme contrast */}
        <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_30%,#000000_100%)] z-10 pointer-events-none opacity-60" />

        {/* Top Edge Highlight */}
        <div className="absolute top-0 left-0 w-full h-[1px] bg-gradient-to-r from-transparent via-white/[0.08] to-transparent z-30 pointer-events-none" />
      </div>

      <div className="relative mx-auto max-w-7xl px-4 sm:px-6 lg:px-8 pt-28 pb-16 lg:pt-36 lg:pb-20 h-full flex items-center">
        <div className="grid lg:grid-cols-2 gap-12 lg:gap-8 items-center w-full">

          {/* Left Content */}
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 1, ease: [0.16, 1, 0.3, 1] }}
            className="max-w-xl z-20"
          >
            <div className="inline-flex items-center gap-2 px-3 py-1.5 rounded-lg bg-white/[0.03] border border-white/[0.08] mb-8">
              <Sparkles size={14} className="text-[#A1A1AA]" />
              <span className="text-xs font-medium text-[#A1A1AA] tracking-wide uppercase">The New Standard for Linux</span>
            </div>

            <h1 className="text-5xl sm:text-7xl lg:text-[84px] font-semibold tracking-tighter text-[#F4F4F5] leading-[1.05] mb-6">
              Capture it.<br />
              <span className="font-serif italic font-normal text-[#71717A]">Beautifully.</span>
            </h1>

            <p className="text-lg text-[#A1A1AA] font-light tracking-wide leading-relaxed max-w-md mb-10">
              The premier screen capture and annotation suite you've been searching for. Absolute precision, wrapped in an obsessively crafted interface.
            </p>

            <div className="flex flex-wrap items-center gap-4">
              <Link
                href="#download"
                className="group flex items-center gap-2 h-14 bg-[#F4F4F5] text-black px-8 rounded-full font-medium transition-all hover:bg-white active:scale-[0.98]"
              >
                Download for Linux
                <ChevronRight size={16} className="group-hover:translate-x-0.5 transition-transform" />
              </Link>

              <Link
                href="#download"
                className="flex items-center gap-2 h-14 bg-transparent border border-white/[0.12] text-[#F4F4F5] px-8 rounded-full font-medium transition-all hover:bg-white/[0.04] active:scale-[0.98]"
              >
                <Command size={18} className="text-[#A1A1AA]" />
                View Manual
              </Link>
            </div>
          </motion.div>

          {/* Right Content - Abstract Isometric Scene */}
          <div className="relative h-[600px] hidden lg:block perspective-[1600px] w-full">
            <motion.div
              initial={{ opacity: 0, rotateX: 25, rotateY: -15, rotateZ: 5, y: 40 }}
              animate={{ opacity: 1, rotateX: 15, rotateY: -20, rotateZ: 8, y: 0 }}
              transition={{ duration: 1.5, ease: [0.16, 1, 0.3, 1] }}
              className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-full h-full transform-style-3d origin-center"
            >

              {/* Back Drop Shadow / Glow to separate from background without being a neon orb */}
              <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[400px] bg-white/[0.02] blur-[80px] rounded-full pointer-events-none" />

              {/* Central Editor Card */}
              <div
                className="absolute top-[10%] left-[10%] w-[540px] rounded-2xl bg-[#0A0A0B] border border-white/[0.08] shadow-[0_40px_80px_rgba(0,0,0,0.8)] overflow-hidden flex flex-col"
                style={{ transform: "translateZ(0px)" }}
              >
                {/* Fake Titlebar */}
                <div className="h-12 border-b border-white/[0.05] flex items-center justify-between px-4 bg-[#0E0E10]">
                  <div className="flex gap-2.5">
                    <div className="w-3 h-3 rounded-full bg-[#27272A]" />
                    <div className="w-3 h-3 rounded-full bg-[#27272A]" />
                    <div className="w-3 h-3 rounded-full bg-[#27272A]" />
                  </div>
                  <div className="text-[11px] font-medium text-[#71717A] tracking-wider">Screenshot_2026-11-04_at_14.42.png</div>
                  <div className="w-10 flex justify-end">
                    <MoreHorizontal size={14} className="text-[#52525B]" />
                  </div>
                </div>
                {/* Main Editor UI */}
                <div className="flex h-[320px]">
                  {/* Toolbar */}
                  <div className="w-[60px] border-r border-white/[0.05] flex flex-col items-center py-4 bg-[#0A0A0B] gap-5 relative z-10">
                    <div className="w-8 h-8 rounded-xl bg-[#F4F4F5] text-black font-semibold flex items-center justify-center text-sm shadow-sm mb-2">A</div>
                    <MousePointer2 size={16} className="text-[#71717A] hover:text-[#F4F4F5] transition-colors cursor-pointer" />
                    <div className="relative">
                      <Crop size={16} className="text-[#F4F4F5] relative z-10" />
                      <div className="absolute -inset-2 bg-white/[0.08] rounded-lg z-0" />
                    </div>
                    <Type size={16} className="text-[#71717A] hover:text-[#F4F4F5] transition-colors cursor-pointer" />
                    <Settings2 size={16} className="text-[#71717A] hover:text-[#F4F4F5] transition-colors cursor-pointer absolute bottom-6" />
                  </div>
                  {/* Canvas */}
                  <div className="flex-1 p-6 bg-[#111113] relative overflow-hidden flex items-center justify-center">
                    {/* Subtle grid pattern inside canvas */}
                    <div className="absolute inset-0" style={{ backgroundImage: 'radial-gradient(#27272A 1px, transparent 1px)', backgroundSize: '16px 16px', opacity: 0.4 }} />

                    <div className="w-full h-full max-h-[240px] max-w-[400px] rounded-xl overflow-hidden border border-white/[0.1] relative shadow-2xl">
                      <img src="https://images.unsplash.com/photo-1618005182384-a83a8bd57fbe?w=800&fit=crop" className="w-full h-full object-cover opacity-90 grayscale-[0.2]" alt="Canvas image" />

                      {/* Crop Overlay Representation */}
                      <div className="absolute inset-0 bg-black/40" />
                      <div className="absolute inset-8 border border-white/40 shadow-[0_0_0_9999px_rgba(0,0,0,0.4)] backdrop-blur-[1px]">
                        {/* Crop handles */}
                        <div className="absolute -top-[3px] -left-[3px] w-6 h-6 border-t-2 border-l-2 border-white" />
                        <div className="absolute -top-[3px] -right-[3px] w-6 h-6 border-t-2 border-r-2 border-white" />
                        <div className="absolute -bottom-[3px] -left-[3px] w-6 h-6 border-b-2 border-l-2 border-white" />
                        <div className="absolute -bottom-[3px] -right-[3px] w-6 h-6 border-b-2 border-r-2 border-white" />
                      </div>
                    </div>
                  </div>
                </div>
              </div>

              {/* Floating Upload Notification Card */}
              <div
                className="absolute bottom-[5%] left-[0%] w-[340px] rounded-2xl bg-[#18181B] border border-white/[0.08] shadow-[0_30px_60px_rgba(0,0,0,0.6)] p-5 flex flex-col gap-4 backdrop-blur-xl"
                style={{ transform: "translateZ(80px)" }}
              >
                <div className="flex items-start gap-4">
                  <div className="w-12 h-12 rounded-xl bg-[#27272A] border border-white/[0.05] flex items-center justify-center shrink-0">
                    <Cloud size={20} className="text-[#F4F4F5]" />
                  </div>
                  <div className="flex flex-col gap-0.5 pt-0.5">
                    <h3 className="text-sm font-semibold text-[#F4F4F5]">Upload Complete</h3>
                    <p className="text-xs text-[#A1A1AA] font-mono tracking-wide">apex.sh/xyz987</p>
                  </div>
                </div>
                <div className="w-full h-px bg-white/[0.06]" />
                <div className="flex justify-between items-center px-1">
                  <span className="text-xs text-[#71717A] font-medium">2.4 MB • PNG</span>
                  <div className="flex items-center gap-1.5 text-xs font-semibold text-black bg-[#F4F4F5] px-3 py-1.5 rounded-full shadow-sm">
                    <Check size={12} strokeWidth={3} /> Copied
                  </div>
                </div>
              </div>

              {/* Floating Recording Widget */}
              <div
                className="absolute top-[25%] -right-[5%] w-[280px] rounded-full bg-[#18181B] border border-white/[0.08] shadow-[0_20px_40px_rgba(0,0,0,0.5)] p-2 pr-5 flex items-center gap-4 backdrop-blur-xl"
                style={{ transform: "translateZ(140px)" }}
              >
                <div className="w-12 h-12 rounded-full bg-red-500 flex items-center justify-center shrink-0">
                  <Square size={14} className="text-white fill-white" />
                </div>
                <div className="flex-1 flex justify-between items-center">
                  <div className="flex flex-col">
                    <span className="text-xs font-semibold text-[#F4F4F5]">Recording...</span>
                    <span className="text-[11px] text-red-500 font-mono font-medium tracking-wider">04:12</span>
                  </div>
                  <div className="w-8 h-8 rounded-full bg-[#27272A] border border-white/[0.05] flex items-center justify-center">
                    <Video size={14} className="text-[#A1A1AA]" />
                  </div>
                </div>
              </div>

            </motion.div>
          </div>
        </div>
      </div>
    </section>
  );
}
