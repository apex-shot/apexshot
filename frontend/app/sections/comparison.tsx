"use client";

import { motion } from "framer-motion";
import { Check, X } from "lucide-react";

const features = [
    "Quick Access Overlay",
    "Advanced Annotation Tools",
    "Screen Recording",
    "Cloud Integration",
    "OCR Text Extraction",
    "Scrolling Capture",
    "Premium UI/UX",
    "Professional Support",
];

const competitors = [
    {
        name: "ApexShot",
        isPrimary: true,
        supports: [true, true, true, true, true, true, true, true],
    },
    {
        name: "Flameshot",
        isPrimary: false,
        supports: [false, true, false, true, false, false, false, false],
    },
    {
        name: "Ksnip",
        isPrimary: false,
        supports: [false, true, false, false, false, false, false, false],
    },
    {
        name: "ScreenRec",
        isPrimary: false,
        supports: [false, false, true, true, false, false, false, false],
    },
];

export function Comparison() {
    return (
        <section id="comparison" className="relative bg-[#000000] py-32 overflow-hidden w-full border-t border-white/[0.05]">

            {/* Background gradients */}
            <div className="absolute inset-0 pointer-events-none">
                <div className="absolute bottom-0 left-1/2 -translate-x-1/2 w-full h-[600px] bg-[radial-gradient(ellipse_at_bottom,rgba(255,255,255,0.02)_0%,transparent_70%)] opacity-80" />
            </div>

            <div className="relative z-10 mx-auto max-w-7xl px-4 sm:px-6 lg:px-8">

                {/* Section Header */}
                <div className="mx-auto max-w-3xl text-center mb-20 lg:mb-24">
                    <motion.div
                        initial={{ opacity: 0, y: 20 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true, margin: "-100px" }}
                        transition={{ duration: 0.8, ease: [0.16, 1, 0.3, 1] }}
                    >
                        <div className="inline-flex items-center gap-2 px-3 py-1.5 rounded-lg bg-white/[0.03] border border-white/[0.08] mb-6">
                            <span className="text-xs font-medium text-[#A1A1AA] tracking-wide uppercase">Why ApexShot?</span>
                        </div>
                        <h2 className="text-4xl md:text-5xl lg:text-6xl font-semibold tracking-tighter text-white leading-[1.1] mb-6">
                            The clear choice for professionals.
                        </h2>
                        <p className="text-lg md:text-xl text-[#A1A1AA] font-light leading-relaxed">
                            Compare ApexShot to the alternatives and see why thousands of Linux users are switching to a more comprehensive tool.
                        </p>
                    </motion.div>
                </div>

                {/* Comparison Table */}
                <div className="max-w-5xl mx-auto">
                    <div className="rounded-3xl bg-[#09090b]/40 border border-white/[0.04] p-1 overflow-hidden shadow-[0_40px_80px_-20px_rgba(0,0,0,0.8)] backdrop-blur-[60px]">
                        <div className="w-full overflow-x-auto">
                            <table className="w-full text-left border-collapse">
                                <thead>
                                    <tr className="border-b border-white/[0.06]">
                                        <th className="p-6 md:p-8 font-medium text-[#A1A1AA] text-sm w-1/3 min-w-[200px] sticky left-0 bg-[#09090b]/40 backdrop-blur-3xl z-10">
                                            Features
                                        </th>
                                        {competitors.map((comp) => (
                                            <th
                                                key={comp.name}
                                                className={`p-6 md:p-8 text-center text-[15px] font-medium min-w-[140px]
                                            ${comp.isPrimary ? "text-white bg-white/[0.03] rounded-t-2xl relative" : "text-[#A1A1AA] font-light"}`}
                                            >
                                                {comp.isPrimary && (
                                                    <div className="absolute top-0 left-0 right-0 h-px bg-gradient-to-r from-transparent via-white/[0.3] to-transparent" />
                                                )}
                                                {comp.name}
                                            </th>
                                        ))}
                                    </tr>
                                </thead>
                                <tbody>
                                    {features.map((feature, featureIdx) => (
                                        <tr key={feature} className="border-b border-white/[0.03] last:border-0 hover:bg-white/[0.01] transition-colors">
                                            <td className="p-6 md:p-8 text-[15px] text-[#A1A1AA] font-light sticky left-0 bg-[#09090b]/40 backdrop-blur-3xl z-10 border-r border-transparent">
                                                {feature}
                                            </td>
                                            {competitors.map((comp) => (
                                                <td
                                                    key={`${comp.name}-${featureIdx}`}
                                                    className={`p-6 md:p-8 text-center ${comp.isPrimary ? 'bg-white/[0.02]' : ''}`}
                                                >
                                                    <div className="flex justify-center">
                                                        {comp.supports[featureIdx] ? (
                                                            <div className={`w-6 h-6 rounded-full flex items-center justify-center ${comp.isPrimary ? 'bg-white' : 'bg-[#18181b] border border-white/[0.05]'}`}>
                                                                <Check size={14} className={comp.isPrimary ? 'text-black' : 'text-[#A1A1AA]'} strokeWidth={3} />
                                                            </div>
                                                        ) : (
                                                            <div className="w-6 h-6 rounded-full flex items-center justify-center">
                                                                <X size={14} className="text-[#3f3f46]" strokeWidth={2} />
                                                            </div>
                                                        )}
                                                    </div>
                                                </td>
                                            ))}
                                        </tr>
                                    ))}
                                </tbody>
                            </table>
                        </div>

                        {/* Subtle bottom highlight for primary column */}
                        <div className="relative h-1 w-full flex">
                            <div className="w-1/3 min-w-[200px]" />
                            <div className="flex-1 min-w-[140px] relative">
                                <div className="absolute bottom-0 left-0 right-0 h-px bg-gradient-to-r from-transparent via-white/[0.3] to-transparent" />
                            </div>
                            <div className="flex-1 min-w-[140px]" />
                            <div className="flex-1 min-w-[140px]" />
                            <div className="flex-1 min-w-[140px]" />
                        </div>
                    </div>
                </div>

            </div>
        </section>
    );
}
