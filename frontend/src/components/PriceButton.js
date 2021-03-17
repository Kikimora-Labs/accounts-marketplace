import React from 'react'
import { fromNear } from './Helpers'
import { Link } from 'react-router-dom'

function AcquireButton (props) {
  return (
    <div>
      <Link
        to={`/acquire/${props.bidId}`}
        className='btn btn-success'
      >
Acquire
      </Link>
    </div>
  )
}

function PriceButton (props) {
  return (
    <div>
      <Link
        to={`/bid/${props.bidId}`}
        className='btn btn-success'
      >
Starts from {(fromNear(props.price) + fromNear(props.forfeit)).toFixed(2)} NEAR
      </Link>
    </div>
  )
}

function DetailsButton (props) {
  return (
    <div>
      <Link
        to={`/bid/${props.bidId}`}
        className='btn btn-primary'
      >
Details
      </Link>
    </div>
  )
}

export { PriceButton, DetailsButton, AcquireButton }