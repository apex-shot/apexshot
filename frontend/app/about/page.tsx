"use client";

import { motion } from "framer-motion";
import { Terminal, Code2, Paintbrush, Heart, MapPin, Zap } from "lucide-react";

export default function AboutPage() {
    return (
        <section className="relative min-h-[120vh] bg-[#000000] flex flex-col items-center pt-32 pb-20 overflow-hidden font-sans w-full cursor-default">

            {/* Background massive 'About' text */}
            <div className="absolute top-[120px] md:top-[140px] left-1/2 -translate-x-1/2 w-full flex justify-center pointer-events-none select-none z-0">
                <h2
                    className="text-[120px] md:text-[200px] lg:text-[280px] font-bold text-white/[0.03] leading-none tracking-tighter"
                >
                    About
                </h2>
            </div>

            <div className="relative z-10 w-full max-w-5xl px-4 mt-32 md:mt-40 lg:mt-48">

                {/* Header Section */}
                <div className="text-center mb-16 md:mb-24 px-4">
                    <motion.div
                        initial={{ opacity: 0, y: 20 }}
                        animate={{ opacity: 1, y: 0 }}
                        transition={{ duration: 0.8, ease: [0.16, 1, 0.3, 1] }}
                    >
                        <h1 className="text-4xl lg:text-5xl font-semibold tracking-tight text-white mb-6">
                            Made for Linux.
                        </h1>
                        <p className="text-[17px] text-[#A1A1AA] font-light leading-relaxed max-w-2xl mx-auto">
                            ApexShot was born from a simple frustration: Linux desktop users deserve beautiful, perfectly crafted software, uncompromised by electron bloat or legacy design paradigms.
                        </p>
                    </motion.div>
                </div>

                <div className="flex flex-col gap-8 md:gap-12 lg:gap-16">

                    {/* The Mission Card */}
                    <motion.div
                        initial={{ opacity: 0, y: 30 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true, margin: "-50px" }}
                        transition={{ duration: 0.8, ease: [0.16, 1, 0.3, 1] }}
                        className="group relative rounded-[32px] bg-[#09090b]/60 backdrop-blur-[60px] border border-white/[0.04] p-8 md:p-14 flex flex-col shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)]"
                    >
                        {/* Inner soft highlight */}
                        <div className="absolute top-0 left-0 right-0 h-px bg-gradient-to-r from-transparent via-white/[0.08] to-transparent pointer-events-none rounded-t-[32px]" />

                        <div className="grid md:grid-cols-2 gap-10 lg:gap-20 items-center">
                            <div className="space-y-6">
                                <h3 className="text-2xl md:text-3xl font-semibold text-white tracking-tight">Our Mission</h3>
                                <p className="text-[16px] text-[#A1A1AA] font-light leading-relaxed">
                                    We believe that productivity tooling shouldn't feel like a chore. The tools we use every day form the environment where we think, work, and create.
                                </p>
                                <p className="text-[16px] text-[#A1A1AA] font-light leading-relaxed">
                                    Our mission is to bring a zero-compromise, premium software experience to the Linux ecosystem. We obsess over milliseconds of latency, precise vector rendering, and incredibly intuitive interfaces so you can focus entirely on what you're capturing.
                                </p>
                            </div>
                            <div className="grid grid-cols-2 gap-6">
                                {/* Values Grid */}
                                <div className="space-y-4">
                                    <div className="w-10 h-10 rounded-full bg-white/[0.05] flex items-center justify-center border border-white/[0.05]">
                                        <Zap size={18} className="text-white" />
                                    </div>
                                    <div>
                                        <h4 className="text-[15px] font-medium text-white tracking-wide">Performance First</h4>
                                        <p className="text-[13px] text-[#71717A] mt-1 font-light">Built natively for speed.</p>
                                    </div>
                                </div>
                                <div className="space-y-4">
                                    <div className="w-10 h-10 rounded-full bg-white/[0.05] flex items-center justify-center border border-white/[0.05]">
                                        <Paintbrush size={18} className="text-white" />
                                    </div>
                                    <div>
                                        <h4 className="text-[15px] font-medium text-white tracking-wide">Obsessive Design</h4>
                                        <p className="text-[13px] text-[#71717A] mt-1 font-light">Every pixel accounted for.</p>
                                    </div>
                                </div>
                                <div className="space-y-4">
                                    <div className="w-10 h-10 rounded-full bg-white/[0.05] flex items-center justify-center border border-white/[0.05]">
                                        <Terminal size={18} className="text-white" />
                                    </div>
                                    <div>
                                        <h4 className="text-[15px] font-medium text-white tracking-wide">Linux Native</h4>
                                        <p className="text-[13px] text-[#71717A] mt-1 font-light">Wayland & X11 ready.</p>
                                    </div>
                                </div>
                                <div className="space-y-4">
                                    <div className="w-10 h-10 rounded-full bg-white/[0.05] flex items-center justify-center border border-white/[0.05]">
                                        <Heart size={18} className="text-white" />
                                    </div>
                                    <div>
                                        <h4 className="text-[15px] font-medium text-white tracking-wide">Community Driven</h4>
                                        <p className="text-[13px] text-[#71717A] mt-1 font-light">Built with your feedback.</p>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </motion.div>

                    {/* The Story / Team Section */}
                    <div className="grid md:grid-cols-2 gap-8 md:gap-12">

                        <motion.div
                            initial={{ opacity: 0, y: 30 }}
                            whileInView={{ opacity: 1, y: 0 }}
                            viewport={{ once: true, margin: "-50px" }}
                            transition={{ duration: 0.8, delay: 0.1, ease: [0.16, 1, 0.3, 1] }}
                            className="group relative rounded-[32px] bg-[#09090b]/60 backdrop-blur-[60px] border border-white/[0.04] p-8 md:p-10 flex flex-col shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)] h-full"
                        >
                            <div className="mb-8">
                                <div className="w-12 h-12 rounded-2xl bg-gradient-to-br from-white/[0.1] to-white/[0.02] flex items-center justify-center border border-white/[0.05] shadow-[inset_0_1px_0_rgba(255,255,255,0.1)] mb-6">
                                    <Code2 size={20} className="text-white" />
                                </div>
                                <h3 className="text-2xl font-semibold text-white tracking-tight mb-4">The Journey</h3>
                                <p className="text-[15px] text-[#A1A1AA] font-light leading-relaxed mb-4">
                                    ApexShot started in late 2025 out of necessity. After cycling through dozens of clunky, outdated screenshot tools on a custom Arch desktop environment, the founder realized there was a massive void in the market.
                                </p>
                                <p className="text-[15px] text-[#A1A1AA] font-light leading-relaxed">
                                    There were fully-featured tools that looked terrible, and minimal tools that did absolutely nothing. ApexShot was designed to exist perfectly in the middle—an extremely powerful editor hidden behind an incredibly clean, unobtrusive interface.
                                </p>
                            </div>
                        </motion.div>

                        <motion.div
                            initial={{ opacity: 0, y: 30 }}
                            whileInView={{ opacity: 1, y: 0 }}
                            viewport={{ once: true, margin: "-50px" }}
                            transition={{ duration: 0.8, delay: 0.2, ease: [0.16, 1, 0.3, 1] }}
                            className="group relative rounded-[32px] bg-[#09090b]/60 backdrop-blur-[60px] border border-white/[0.04] p-8 md:p-10 flex flex-col shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)] h-full"
                        >
                            <div className="mb-8 flex-1">
                                <div className="w-12 h-12 rounded-2xl bg-gradient-to-br from-white/[0.1] to-white/[0.02] flex items-center justify-center border border-white/[0.05] shadow-[inset_0_1px_0_rgba(255,255,255,0.1)] mb-6">
                                    <MapPin size={20} className="text-white" />
                                </div>
                                <h3 className="text-2xl font-semibold text-white tracking-tight mb-4">A Global Team</h3>
                                <p className="text-[15px] text-[#A1A1AA] font-light leading-relaxed mb-4">
                                    While the project began as a one-person endeavor, it quickly gathered interest from open-source contributors and designers who shared the same vision.
                                </p>
                                <p className="text-[15px] text-[#A1A1AA] font-light leading-relaxed mb-6">
                                    Today, ApexShot is maintained by a small, obsessive core group of 5 engineers and 2 UI/UX designers spread across the globe, united by a love for quality desktop applications.
                                </p>
                            </div>
                        </motion.div>

                    </div>
                </div>

                {/* Bottom spacer block to fade out nicely */}
                <div className="mt-20 flex justify-center pb-20">
                    <div className="w-2 h-2 rounded-full bg-white/[0.1] shadow-xl" />
                </div>
            </div>
        </section>
    );
}
