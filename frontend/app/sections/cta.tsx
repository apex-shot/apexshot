"use client";

import { motion } from "framer-motion";
import { ChevronRight } from "lucide-react";
import Link from "next/link";

export function CTA() {
    return (
        <section className="relative bg-[#000000] py-32 overflow-hidden w-full border-t border-white/[0.05]">

            {/* Heavy central glow */}
            <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-full max-w-[800px] h-[600px] bg-[radial-gradient(ellipse_at_center,rgba(255,255,255,0.05)_0%,transparent_70%)] opacity-80 pointer-events-none" />

            {/* Grid pattern overlay */}
            <div className="absolute inset-0 pointer-events-none z-0">
                <svg className="absolute w-full h-full opacity-[0.08]" xmlns="http://www.w3.org/2000/svg">
                    <defs>
                        <pattern id="cta-grid" width="40" height="40" patternUnits="userSpaceOnUse">
                            <path d="M 40 0 L 0 0 0 40" fill="none" stroke="white" strokeWidth="0.5" strokeDasharray="1 3" />
                        </pattern>
                    </defs>
                    <rect width="100%" height="100%" fill="url(#cta-grid)" />
                </svg>
            </div>

            <div className="relative z-10 mx-auto max-w-4xl px-4 sm:px-6 lg:px-8 text-center">

                <motion.div
                    initial={{ opacity: 0, scale: 0.95, y: 30 }}
                    whileInView={{ opacity: 1, scale: 1, y: 0 }}
                    viewport={{ once: true, margin: "-100px" }}
                    transition={{ duration: 0.8, ease: [0.16, 1, 0.3, 1] }}
                    className="bg-[#09090b]/40 backdrop-blur-[60px] border border-white/[0.04] rounded-[40px] p-12 md:p-20 shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)] overflow-hidden relative group"
                >
                    {/* Subtle top edge highlight */}
                    <div className="absolute top-0 left-0 right-0 h-px bg-gradient-to-r from-transparent via-white/[0.2] to-transparent opacity-80" />

                    <div className="relative z-10">
                        <h2 className="text-4xl md:text-5xl lg:text-6xl font-semibold tracking-tighter text-white leading-[1.1] mb-6">
                            Ready to Upgrade Your Workflow?
                        </h2>
                        <p className="text-lg md:text-xl text-[#A1A1AA] font-light leading-relaxed max-w-2xl mx-auto mb-12">
                            Join thousands of Linux users who have already made the switch to ApexShot. Experience the absolute best screen capture tool built natively for your OS.
                        </p>

                        <div className="flex flex-col sm:flex-row items-center justify-center gap-4">
                            <Link
                                href="/pricing"
                                className="group flex items-center justify-center gap-2 h-14 bg-[#F4F4F5] text-black px-10 rounded-full font-medium transition-all hover:bg-white active:scale-[0.98] w-full sm:w-auto"
                            >
                                Get Started Today
                                <ChevronRight size={16} className="group-hover:translate-x-0.5 transition-transform" />
                            </Link>
                            <Link
                                href="#download"
                                className="flex items-center justify-center h-14 bg-transparent border border-white/10 text-[#F4F4F5] px-10 rounded-full font-medium transition-colors hover:bg-white/[0.05] w-full sm:w-auto"
                            >
                                Download Free Trial
                            </Link>
                        </div>
                    </div>

                    {/* Subtle animated background gradient element just inside the card */}
                    <motion.div
                        className="absolute -bottom-[50%] -right-[50%] w-full h-full bg-[radial-gradient(circle_at_center,rgba(255,255,255,0.08)_0%,transparent_60%)] z-0 rounded-full"
                        animate={{
                            x: [0, 50, 0],
                            y: [0, -50, 0]
                        }}
                        transition={{ duration: 10, repeat: Infinity, ease: "easeInOut" }}
                    />
                </motion.div>

            </div>
        </section>
    );
}
