import {useState} from 'react'

/** Copy `text` to the clipboard and flash `true` for `ms`. */
export function useCopy(ms = 1500): [boolean, (text: string) => void] {
    const [copied, setCopied] = useState(false)

    function copy(text: string) {
        void navigator.clipboard?.writeText(text)
        setCopied(true)
        window.setTimeout(() => setCopied(false), ms)
    }

    return [copied, copy]
}
