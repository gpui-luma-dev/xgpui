//! One settings screen in several locales. Sample translations for testing
//! layout, not reviewed by native speakers.

/// The screen's strings in one locale.
pub(crate) struct Locale {
    /// BCP 47 tag, shown in the panel header.
    pub tag: &'static str,
    /// The language's own name.
    pub name: &'static str,
    /// Whether the UI mirrors (right-to-left languages).
    pub rtl: bool,
    pub title: &'static str,
    /// Setting labels and their values.
    pub rows: [(&'static str, &'static str); 3],
    pub description: &'static str,
    pub buttons: [&'static str; 2],
    pub path: &'static str,
}

pub(crate) const LOCALES: &[Locale] = &[
    Locale {
        tag: "en",
        name: "English",
        rtl: false,
        title: "Privacy and security",
        rows: [
            ("Location services", "On"),
            ("Share usage data with developers", "Off"),
            ("Automatic updates", "Daily"),
        ],
        description: "Changes to these settings apply to all devices signed in to your account. Some apps may need to restart before the new settings take effect.",
        buttons: ["Cancel", "Save changes"],
        path: "Settings › Privacy and security › Location services",
    },
    Locale {
        tag: "de",
        name: "Deutsch",
        rtl: false,
        title: "Datenschutz und Sicherheit",
        rows: [
            ("Ortungsdienste", "Ein"),
            ("Nutzungsdaten mit Entwicklern teilen", "Aus"),
            ("Aktualisierungsbenachrichtigungen", "Täglich"),
        ],
        description: "Änderungen an diesen Einstellungen gelten für alle Geräte, die bei Ihrem Konto angemeldet sind. Sicherheitsrichtlinienverwaltungsoptionen werden erst nach einem Neustart übernommen.",
        buttons: ["Abbrechen", "Änderungen speichern"],
        path: "Einstellungen › Datenschutz und Sicherheit › Ortungsdienste",
    },
    Locale {
        tag: "ar",
        name: "العربية",
        rtl: true,
        title: "الخصوصية والأمان",
        rows: [
            ("خدمات الموقع", "تشغيل"),
            ("مشاركة بيانات الاستخدام مع المطورين", "إيقاف"),
            ("التحديثات التلقائية", "يومياً"),
        ],
        description: "تطبق التغييرات على هذه الإعدادات على جميع الأجهزة المسجلة في حسابك. قد تحتاج بعض التطبيقات إلى إعادة التشغيل قبل أن تسري الإعدادات الجديدة.",
        buttons: ["إلغاء", "حفظ التغييرات"],
        path: "الإعدادات › الخصوصية والأمان › خدمات الموقع",
    },
    Locale {
        tag: "he",
        name: "עברית",
        rtl: true,
        title: "פרטיות ואבטחה",
        rows: [
            ("שירותי מיקום", "פועל"),
            ("שיתוף נתוני שימוש עם המפתחים", "כבוי"),
            ("עדכונים אוטומטיים", "יומי"),
        ],
        description: "השינויים בהגדרות אלה חלים על כל המכשירים המחוברים לחשבון שלך. ייתכן שחלק מהאפליקציות יצטרכו להפעיל מחדש לפני שההגדרות החדשות ייכנסו לתוקף.",
        buttons: ["ביטול", "שמירת השינויים"],
        path: "הגדרות › פרטיות ואבטחה › שירותי מיקום",
    },
    Locale {
        tag: "th",
        name: "ไทย",
        rtl: false,
        title: "ความเป็นส่วนตัวและความปลอดภัย",
        rows: [
            ("บริการหาตำแหน่ง", "เปิด"),
            ("แชร์ข้อมูลการใช้งานกับนักพัฒนา", "ปิด"),
            ("อัปเดตอัตโนมัติ", "ทุกวัน"),
        ],
        description: "การเปลี่ยนแปลงการตั้งค่าเหล่านี้จะมีผลกับอุปกรณ์ทั้งหมดที่ลงชื่อเข้าใช้บัญชีของคุณ แอปบางแอปอาจต้องเริ่มต้นใหม่ก่อนที่การตั้งค่าใหม่จะมีผล",
        buttons: ["ยกเลิก", "บันทึกการเปลี่ยนแปลง"],
        path: "การตั้งค่า › ความเป็นส่วนตัวและความปลอดภัย › บริการหาตำแหน่ง",
    },
    Locale {
        tag: "km",
        name: "ខ្មែរ",
        rtl: false,
        title: "ឯកជនភាព និងសុវត្ថិភាព",
        rows: [
            ("សេវាកម្មទីតាំង", "បើក"),
            ("ចែករំលែកទិន្នន័យប្រើប្រាស់ជាមួយអ្នកអភិវឌ្ឍន៍", "បិទ"),
            ("ការធ្វើបច្ចុប្បន្នភាពស្វ័យប្រវត្តិ", "រាល់ថ្ងៃ"),
        ],
        description: "ការផ្លាស់ប្តូរការកំណត់ទាំងនេះអនុវត្តចំពោះឧបករណ៍ទាំងអស់ដែលបានចូលគណនីរបស់អ្នក។ កម្មវិធីមួយចំនួនអាចត្រូវចាប់ផ្តើមឡើងវិញមុនពេលការកំណត់ថ្មីមានប្រសិទ្ធភាព។",
        buttons: ["បោះបង់", "រក្សាទុកការផ្លាស់ប្តូរ"],
        path: "ការកំណត់ › ឯកជនភាព និងសុវត្ថិភាព › សេវាកម្មទីតាំង",
    },
    Locale {
        tag: "ja",
        name: "日本語",
        rtl: false,
        title: "プライバシーとセキュリティ",
        rows: [
            ("位置情報サービス", "オン"),
            ("使用状況データを開発者と共有", "オフ"),
            ("自動アップデート", "毎日"),
        ],
        description: "これらの設定の変更は、アカウントにサインインしているすべてのデバイスに適用されます。新しい設定を有効にするには、一部のアプリの再起動が必要な場合があります。",
        buttons: ["キャンセル", "変更を保存"],
        path: "設定 › プライバシーとセキュリティ › 位置情報サービス",
    },
    Locale {
        tag: "hi",
        name: "हिन्दी",
        rtl: false,
        title: "गोपनीयता और सुरक्षा",
        rows: [
            ("स्थान सेवाएँ", "चालू"),
            ("डेवलपर्स के साथ उपयोग डेटा साझा करें", "बंद"),
            ("स्वचालित अपडेट", "रोज़ाना"),
        ],
        description: "इन सेटिंग्स में बदलाव आपके खाते में साइन इन किए गए सभी उपकरणों पर लागू होते हैं। नई सेटिंग्स लागू होने से पहले कुछ ऐप्स को फिर से शुरू करना पड़ सकता है।",
        buttons: ["रद्द करें", "बदलाव सहेजें"],
        path: "सेटिंग्स › गोपनीयता और सुरक्षा › स्थान सेवाएँ",
    },
];
