/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package mozilla.appservices.fxaclient

data class Device(
    val id: String,
    val displayName: String,
    val deviceType: Type,
    val pushSubscription: PushSubscription?,
    val pushEndpointExpired: Boolean,
    val isCurrentDevice: Boolean,
    val lastAccessTime: Long?
) {
    enum class Type {
        DESKTOP,
        MOBILE,
        UNKNOWN;

        companion object {
            internal fun fromMessage(msg: MsgTypes.Device.Type): Type {
                return when (msg) {
                    MsgTypes.Device.Type.DESKTOP -> DESKTOP
                    MsgTypes.Device.Type.MOBILE -> MOBILE
                    else -> UNKNOWN
                }
            }
        }
    }
    data class PushSubscription(
        val endpoint: String,
        val publicKey: String,
        val authKey: String
    ) {
        companion object {
            internal fun fromMessage(msg: MsgTypes.Device.PushSubscription): PushSubscription {
                return PushSubscription(
                        endpoint = msg.endpoint,
                        publicKey = msg.publicKey,
                        authKey = msg.authKey
                )
            }
        }
    }
    companion object {
        internal fun fromMessage(msg: MsgTypes.Device): Device {
            return Device(
                    id = msg.id,
                    displayName = msg.displayName,
                    deviceType = Type.fromMessage(msg.type),
                    pushSubscription = if (msg.hasPushSubscription()) {
                        PushSubscription.fromMessage(msg.pushSubscription)
                    } else null,
                    pushEndpointExpired = msg.pushEndpointExpired,
                    isCurrentDevice = msg.isCurrentDevice,
                    lastAccessTime = if (msg.hasLastAccessTime()) msg.lastAccessTime else null
            )
        }
        internal fun fromCollectionMessage(msg: MsgTypes.Devices): Array<Device> {
            return msg.devicesList.map {
                fromMessage(it)
            }.toTypedArray()
        }
    }
}
