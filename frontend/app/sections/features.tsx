"use client";

import { motion } from "framer-motion";
import { Camera, PenTool, Video, Cloud, Type, Image as ImageIcon } from "lucide-react";

const features = [
    {
        title: "Instant Capture",
        description: "Lightning-fast screenshots of your entire screen, a specific window, or any custom area with pixel-perfect precision.",
        icon: Camera,
    },
    {
        title: "Built-in Annotation",
        description: "Professional markdown and drawing tools. Add arrows, boxes, text, blur sensitive info, or pixelate with ease.",
        icon: PenTool,
    },
    {
        title: "Screen Recording",
        description: "Capture fluid MP4 and GIF recordings. Automatically highlight clicks and capture keystrokes for perfect tutorials.",
        icon: Video,
    },
    {
        title: "Cloud Sharing",
        description: "One-click upload to the cloud. Instantly get a shareable link copied to your clipboard, ready to drop into Slack or Discord.",
        icon: Cloud,
    },
    {
        title: "Text Recognition",
        description: "Powerful on-device OCR instantly extracts selectable text from any image or screenshot without leaving your machine.",
        icon: Type,
    },
    {
        title: "Beautiful Backgrounds",
        description: "Elevate your screenshots. Automatically add professional backgrounds, elegant padding, and gorgeous drop shadows.",
        icon: ImageIcon,
    },
];

export function Features() {
    return (
        <section id="features" className="relative bg-[#000000] py-32 overflow-hidden w-full border-t border-white/[0.05]">

            {/* Subtle Background Elements */}
            <div className="absolute inset-0 pointer-events-none">
                <div className="absolute top-0 left-1/2 -translate-x-1/2 w-full h-[600px] bg-[radial-gradient(ellipse_at_top,rgba(255,255,255,0.02)_0%,transparent_70%)] opacity-80" />
            </div>

            <div className="relative z-10 mx-auto max-w-7xl px-4 sm:px-6 lg:px-8">

                {/* Section Header */}
                <div className="max-w-3xl mb-24">
                    <motion.div
                        initial={{ opacity: 0, y: 20 }}
                        whileInView={{ opacity: 1, y: 0 }}
                        viewport={{ once: true, margin: "-100px" }}
                        transition={{ duration: 0.8, ease: [0.16, 1, 0.3, 1] }}
                    >
                        <div className="inline-flex items-center gap-2 px-3 py-1.5 rounded-lg bg-white/[0.03] border border-white/[0.08] mb-6">
                            <span className="text-xs font-medium text-[#A1A1AA] tracking-wide uppercase">Powerful Features</span>
                        </div>
                        <h2 className="text-4xl md:text-5xl lg:text-6xl font-semibold tracking-tighter text-white leading-[1.1] mb-6">
                            Everything you need to capture and communicate.
                        </h2>
                        <p className="text-lg md:text-xl text-[#A1A1AA] font-light leading-relaxed">
                            ApexShot is engineered for speed and precision. We've packed it with professional tools so you rarely ever need to open an external image editor.
                        </p>
                    </motion.div>
                </div>

                {/* Features Grid */}
                <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-6 lg:gap-8">
                    {features.map((feature, index) => (
                        <motion.div
                            key={feature.title}
                            initial={{ opacity: 0, y: 20 }}
                            whileInView={{ opacity: 1, y: 0 }}
                            viewport={{ once: true, margin: "-50px" }}
                            transition={{ duration: 0.8, delay: index * 0.1, ease: [0.16, 1, 0.3, 1] }}
                            className="group relative rounded-3xl bg-[#09090b]/40 border border-white/[0.04] p-8 flex flex-col hover:bg-[#09090b]/60 transition-colors duration-500 overflow-hidden"
                        >
                            <div className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-white/[0.08] to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500" />

                            <div className="w-12 h-12 rounded-2xl bg-white/[0.03] border border-white/[0.05] flex items-center justify-center mb-8 group-hover:scale-110 group-hover:bg-white/[0.08] transition-all duration-500 ease-out">
                                <feature.icon size={22} className="text-[#F4F4F5]" strokeWidth={1.5} />
                            </div>

                            <h3 className="text-xl font-medium text-white tracking-tight mb-3">
                                {feature.title}
                            </h3>

                            <p className="text-[15px] text-[#A1A1AA] font-light leading-relaxed">
                                {feature.description}
                            </p>
                        </motion.div>
                    ))}
                </div>
            </div>
        </section>
    );
}
